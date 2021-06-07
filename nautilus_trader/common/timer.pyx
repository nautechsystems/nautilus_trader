# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

from libc.stdint cimport int64_t

from threading import Timer as TimerThread

from cpython.datetime cimport datetime

from nautilus_trader.common.timer cimport TimeEvent
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport format_iso8601
from nautilus_trader.core.datetime cimport nanos_to_secs
from nautilus_trader.core.datetime cimport nanos_to_unix_dt
from nautilus_trader.core.message cimport Event
from nautilus_trader.core.uuid cimport UUID


cdef class TimeEvent(Event):
    """
    Represents a time event occurring at the event timestamp.
    """

    def __init__(
        self,
        str name not None,
        UUID event_id not None,
        datetime event_timestamp not None,
        int64_t event_timestamp_ns,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the ``TimeEvent`` class.

        Parameters
        ----------
        name : str
            The event label.
        event_id : UUID
            The event identifier.
        event_timestamp : datetime
            The event timestamp (UTC).
        event_timestamp_ns : int64
            The UNIX timestamp (nanos) of the event.
        timestamp_ns : int64
            The UNIX timestamp (nanos) of the event initialization.

        """
        Condition.valid_string(name, "name")
        super().__init__(event_id, timestamp_ns)

        self.name = name
        self.event_timestamp = event_timestamp
        self.event_timestamp_ns = event_timestamp_ns

    def __eq__(self, TimeEvent other) -> bool:
        return self.name == other.name

    def __ne__(self, TimeEvent other) -> bool:
        return self.name != other.name

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"name={self.name}, "
                f"id={self.id}, "
                f"event_timestamp={format_iso8601(self.event_timestamp)})")


cdef class TimeEventHandler:
    """
    Represents a bundled event and handler.
    """

    def __init__(self, TimeEvent event not None, handler not None: callable):
        self.event = event
        self._handler = handler

    def handle_py(self) -> None:
        """
        Python wrapper for testing.
        """
        self.handle()

    cdef void handle(self) except *:
        self._handler(self.event)

    def __eq__(self, TimeEventHandler other) -> bool:
        return self.event.event_timestamp_ns == other.event.event_timestamp_ns

    def __ne__(self, TimeEventHandler other) -> bool:
        return self.event.event_timestamp_ns != other.event.event_timestamp_ns

    def __lt__(self, TimeEventHandler other) -> bool:
        return self.event.event_timestamp_ns < other.event.event_timestamp_ns

    def __le__(self, TimeEventHandler other) -> bool:
        return self.event.event_timestamp_ns <= other.event.event_timestamp_ns

    def __gt__(self, TimeEventHandler other) -> bool:
        return self.event.event_timestamp_ns > other.event.event_timestamp_ns

    def __ge__(self, TimeEventHandler other) -> bool:
        return self.event.event_timestamp_ns >= other.event.event_timestamp_ns

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"event={self.event})")


cdef class Timer:
    """
    The abstract base class for all timers.

    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        str name not None,
        callback not None: callable,
        int64_t interval_ns,
        int64_t start_time_ns,
        int64_t stop_time_ns=0,
    ):
        """
        Initialize a new instance of the ``Timer`` class.

        Parameters
        ----------
        name : str
            The name for the timer.
        callback : callable
            The function to call at the next time.
        interval_ns : int64
            The time interval for the timer (not negative).
        start_time_ns : int64
            The UNIX time (nanoseconds) for timer start.
        stop_time_ns : int64, optional
            The UNIX time (nanoseconds) for timer stop (if 0 then timer is continuous).

        """
        Condition.valid_string(name, "name")
        Condition.callable(callback, "function")
        Condition.positive_int64(interval_ns, "interval_ns")

        self.name = name
        self.callback = callback

        # Note that for very large time intervals (greater than 270 years on
        # most platforms) the below will lose microsecond accuracy.
        self.interval_ns = interval_ns
        self.start_time_ns = start_time_ns
        self.next_time_ns = start_time_ns + interval_ns
        self.stop_time_ns = stop_time_ns

        self.is_expired = False

    def __eq__(self, Timer other) -> bool:
        return self.name == other.name

    def __ne__(self, Timer other) -> bool:
        return self.name != other.name

    def __hash__(self) -> int:
        return hash(self.name)

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"name={self.name}, "
                f"interval_ns={self.interval_ns}, "
                f"start_time_ns={self.start_time_ns}, "
                f"next_time_ns={self.next_time_ns}, "
                f"stop_time_ns={self.stop_time_ns}, "
                f"is_expired={self.is_expired})")

    cpdef TimeEvent pop_event(self, UUID event_id, int64_t timestamp_ns):
        """
        Return a generated time event with the given identifier.

        Parameters
        ----------
        event_id : UUID
            The identifier for the time event.
        timestamp_ns : int64
            The UNIX timestamp (nanos) for time event initialization.

        Returns
        -------
        TimeEvent

        """
        Condition.not_none(event_id, "event_id")

        return TimeEvent(
            name=self.name,
            event_id=event_id,
            event_timestamp=nanos_to_unix_dt(nanos=self.next_time_ns),
            event_timestamp_ns=self.next_time_ns,
            timestamp_ns=timestamp_ns,
        )

    cpdef void iterate_next_time(self, int64_t now_ns) except *:
        """
        Iterates the timers next time and checks if the timer is now expired.

        Parameters
        ----------
        now_ns : int64
            The UNIX time now (nanoseconds).

        """
        self.next_time_ns += self.interval_ns
        if self.stop_time_ns and now_ns >= self.stop_time_ns:
            self.is_expired = True

    cpdef void cancel(self) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")


cdef class TestTimer(Timer):
    """
    Provides a fake timer for backtesting and unit testing.
    """
    __test__ = False

    def __init__(
        self,
        str name not None,
        callback not None: callable,
        int64_t interval_ns,
        int64_t start_time_ns,
        int64_t stop_time_ns=0,
    ):
        """
        Initialize a new instance of the ``TestTimer`` class.

        Parameters
        ----------
        name : str
            The name for the timer.
        callback : callable
            The function to call at the next time.
        interval_ns : int64
            The time interval for the timer (not negative).
        start_time_ns : int64
            The UNIX time (nanoseconds) for timer start.
        stop_time_ns : int64, optional
            The UNIX time (nanoseconds) for timer stop (if 0 then timer is continuous).

        """
        Condition.valid_string(name, "name")
        super().__init__(
            name=name,
            callback=callback,
            interval_ns=interval_ns,
            start_time_ns=start_time_ns,
            stop_time_ns=stop_time_ns,
        )

        self._uuid_factory = UUIDFactory()

    cpdef list advance(self, int64_t to_time_ns):
        """
        Advance the test timer forward to the given time, generating a sequence
        of events. A ``TimeEvent`` is appended for each time a next event is
        <= the given to_time.

        Parameters
        ----------
        to_time_ns : int64
            The UNIX time (nanoseconds) to advance the timer to.

        Returns
        -------
        list[TimeEvent]

        """
        cdef list events = []  # type: list[TimeEvent]
        while not self.is_expired and to_time_ns >= self.next_time_ns:
            events.append(self.pop_event(
                event_id=self._uuid_factory.generate(),
                timestamp_ns=self.next_time_ns,
            ))
            self.iterate_next_time(to_time_ns=self.next_time_ns)

        return events

    cpdef Event pop_next_event(self):
        """
        Return the next time event for this timer.

        Returns
        -------
        TimeEvent

        Raises
        ------
        ValueError
            If the next event timestamp is not equal to the at_time.

        """
        cdef TimeEvent event = self.pop_event(
            event_id=self._uuid_factory.generate(),
            timestamp_ns=self.next_time_ns,
        )
        self.iterate_next_time(to_time_ns=self.next_time_ns)

        return event

    cpdef void cancel(self) except *:
        """
        Cancels the timer (the timer will not generate an event).
        """
        self.is_expired = True


cdef class LiveTimer(Timer):
    """
    The abstract base class for all live timers.

    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        str name not None,
        callback not None: callable,
        int64_t interval_ns,
        int64_t now_ns,
        int64_t start_time_ns,
        int64_t stop_time_ns=0,
    ):
        """
        Initialize a new instance of the ``LiveTimer`` class.

        Parameters
        ----------
        name : str
            The name for the timer.
        callback : callable
            The function to call at the next time.
        interval_ns : int64
            The time interval for the timer.
        now_ns : int64
            The datetime now (UTC).
        start_time_ns : int64
            The start datetime for the timer (UTC).
        stop_time_ns : int64, optional
            The stop datetime for the timer (UTC) (if None then timer repeats).

        Raises
        ------
        TypeError
            If callback is not of type callable.

        """
        Condition.valid_string(name, "name")
        super().__init__(
            name=name,
            callback=callback,
            interval_ns=interval_ns,
            start_time_ns=start_time_ns,
            stop_time_ns=stop_time_ns,
        )

        self._internal = self._start_timer(now_ns)

    cpdef void repeat(self, int64_t now_ns) except *:
        """
        Continue the timer.

        Parameters
        ----------
        now_ns : int64
            The current time to continue timing from.

        """
        self._internal = self._start_timer(now_ns)

    cpdef void cancel(self) except *:
        """
        Cancels the timer (the timer will not generate an event).
        """
        self._internal.cancel()

    cdef object _start_timer(self, int64_t now_ns):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")


cdef class ThreadTimer(LiveTimer):
    """
    Provides a thread based timer for live trading.
    """

    def __init__(
        self,
        str name not None,
        callback not None: callable,
        int64_t interval_ns,
        int64_t now_ns,
        int64_t start_time_ns,
        int64_t stop_time_ns=0,
    ):
        """
        Initialize a new instance of the ``LiveTimer`` class.

        Parameters
        ----------
        name : str
            The name for the timer.
        callback : callable
            The function to call at the next time.
        interval_ns : int64
            The time interval for the timer.
        now_ns : int64
            The datetime now (UTC).
        start_time_ns : int64
            The start datetime for the timer (UTC).
        stop_time_ns : int64, optional
            The stop datetime for the timer (UTC) (if None then timer repeats).

        Raises
        ------
        TypeError
            If callback is not of type callable.

        """
        super().__init__(
            name=name,
            callback=callback,
            interval_ns=interval_ns,
            now_ns=now_ns,
            start_time_ns=start_time_ns,
            stop_time_ns=stop_time_ns,
        )

    cdef object _start_timer(self, int64_t now_ns):
        timer = TimerThread(
            interval=nanos_to_secs(self.next_time_ns - now_ns),
            function=self.callback,
            args=[self],
        )
        timer.daemon = True
        timer.start()

        return timer


cdef class LoopTimer(LiveTimer):
    """
    Provides an event loop based timer for live trading.
    """

    def __init__(
        self,
        loop not None,
        str name not None,
        callback not None: callable,
        int64_t interval_ns,
        int64_t now_ns,
        int64_t start_time_ns,
        int64_t stop_time_ns=0,
    ):
        """
        Initialize a new instance of the ``LoopTimer`` class.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop to run the timer on.
        name : str
            The name for the timer.
        callback : callable
            The function to call at the next time.
        interval_ns : int64
            The time interval for the timer.
        now_ns : int64
            The datetime now (UTC).
        start_time_ns : int64
            The start datetime for the timer (UTC).
        stop_time_ns : int64, optional
            The stop datetime for the timer (UTC) (if None then timer repeats).

        Raises
        ------
        TypeError
            If callback is not of type callable.

        """
        Condition.valid_string(name, "name")

        self._loop = loop  # Assign here as `super().__init__` will call it
        super().__init__(
            name=name,
            callback=callback,
            interval_ns=interval_ns,
            now_ns=now_ns,
            start_time_ns=start_time_ns,
            stop_time_ns=stop_time_ns,
        )

    cdef object _start_timer(self, int64_t now_ns):
        return self._loop.call_later(
            nanos_to_secs(self.next_time_ns - now_ns),
            self.callback,
            self,
        )
