# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

from typing import Callable

from libc.stdint cimport uint64_t

from threading import Timer as TimerThread

from nautilus_trader.common.timer cimport TimeEvent
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport nanos_to_secs
from nautilus_trader.core.message cimport Event
from nautilus_trader.core.uuid cimport UUID4


cdef class TimeEvent(Event):
    """
    Represents a time event occurring at the event timestamp.

    Parameters
    ----------
    name : str
        The event name.
    event_id : UUID4
        The event ID.
    ts_event : uint64_t
        The UNIX timestamp (nanoseconds) when the time event occurred.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the object was initialized.
    """

    def __init__(
        self,
        str name not None,
        UUID4 event_id not None,
        uint64_t ts_event,
        uint64_t ts_init,
    ):
        Condition.valid_string(name, "name")
        super().__init__(event_id, ts_event, ts_init)

        self.name = name

    def __eq__(self, TimeEvent other) -> bool:
        return self.name == other.name

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"name={self.name}, "
            f"id={self.id})"
        )


cdef class TimeEventHandler:
    """
    Represents a bundled event and handler.
    """

    def __init__(
        self,
        TimeEvent event not None,
        handler not None: Callable[[TimeEvent], None],
    ):
        self.event = event
        self._handler = handler

    def handle_py(self) -> None:
        """
        Python wrapper for testing.
        """
        self.handle()

    cpdef void handle(self) except *:
        self._handler(self.event)

    def __eq__(self, TimeEventHandler other) -> bool:
        return self.event.ts_event == other.event.ts_event

    def __lt__(self, TimeEventHandler other) -> bool:
        return self.event.ts_event < other.event.ts_event

    def __le__(self, TimeEventHandler other) -> bool:
        return self.event.ts_event <= other.event.ts_event

    def __gt__(self, TimeEventHandler other) -> bool:
        return self.event.ts_event > other.event.ts_event

    def __ge__(self, TimeEventHandler other) -> bool:
        return self.event.ts_event >= other.event.ts_event

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"event={self.event})"
        )


cdef class Timer:
    """
    The abstract base class for all timers.

    Parameters
    ----------
    name : str
        The name for the timer.
    callback : Callable[[TimeEvent], None]
        The delegate to call at the next time.
    interval_ns : uint64_t
        The time interval for the timer (not negative).
    start_time_ns : uint64_t
        The UNIX time (nanoseconds) for timer start.
    stop_time_ns : uint64_t, optional
        The UNIX time (nanoseconds) for timer stop (if 0 then timer is continuous).

    Raises
    ------
    ValueError
        If `name` is not a valid string.
    TypeError
        If `callback` is not of type `Callable`.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        str name not None,
        callback not None: Callable[[TimeEvent], None],
        uint64_t interval_ns,
        uint64_t start_time_ns,
        uint64_t stop_time_ns=0,
    ):
        Condition.valid_string(name, "name")
        Condition.callable(callback, "callback")

        self.name = name
        self.callback = callback
        self.interval_ns = interval_ns
        self.start_time_ns = start_time_ns
        self.next_time_ns = start_time_ns + interval_ns
        self.stop_time_ns = stop_time_ns
        self.is_expired = False

    def __eq__(self, Timer other) -> bool:
        return self.name == other.name

    def __hash__(self) -> int:
        return hash(self.name)

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"name={self.name}, "
            f"interval_ns={self.interval_ns}, "
            f"start_time_ns={self.start_time_ns}, "
            f"next_time_ns={self.next_time_ns}, "
            f"stop_time_ns={self.stop_time_ns}, "
            f"is_expired={self.is_expired})"
        )

    cpdef TimeEvent pop_event(self, UUID4 event_id, uint64_t ts_init):
        """
        Return a generated time event with the given ID.

        Parameters
        ----------
        event_id : UUID4
            The ID for the time event.
        ts_init : uint64_t
            The UNIX timestamp (nanoseconds) when the object was initialized.

        Returns
        -------
        TimeEvent

        """
        Condition.not_none(event_id, "event_id")

        return TimeEvent(
            name=self.name,
            event_id=event_id,
            ts_event=self.next_time_ns,
            ts_init=ts_init,
        )

    cpdef void iterate_next_time(self, uint64_t now_ns) except *:
        """
        Iterates the timers next time and checks if the timer is now expired.

        Parameters
        ----------
        now_ns : uint64_t
            The UNIX time now (nanoseconds).

        """
        self.next_time_ns += self.interval_ns
        if self.stop_time_ns and now_ns >= self.stop_time_ns:
            self.is_expired = True

    cpdef void cancel(self) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover


cdef class TestTimer(Timer):
    """
    Provides a fake timer for backtesting and unit testing.

    Parameters
    ----------
    name : str
        The name for the timer.
    callback : Callable[[TimeEvent], None]
        The delegate to call at the next time.
    interval_ns : uint64_t
        The time interval for the timer (not negative).
    start_time_ns : uint64_t
        The UNIX time (nanoseconds) for timer start.
    stop_time_ns : uint64_t, optional
        The UNIX time (nanoseconds) for timer stop (if 0 then timer is continuous).
    """
    __test__ = False

    def __init__(
        self,
        str name not None,
        callback not None: Callable[[TimeEvent], None],
        uint64_t interval_ns,
        uint64_t start_time_ns,
        uint64_t stop_time_ns=0,
    ):
        Condition.valid_string(name, "name")
        super().__init__(
            name=name,
            callback=callback,
            interval_ns=interval_ns,
            start_time_ns=start_time_ns,
            stop_time_ns=stop_time_ns,
        )

    cpdef list advance(self, uint64_t to_time_ns):
        """
        Advance the test timer forward to the given time, generating a sequence
        of events. A ``TimeEvent`` is appended for each time a next event is
        <= the given to_time.

        Parameters
        ----------
        to_time_ns : uint64_t
            The UNIX time (nanoseconds) to advance the timer to.

        Returns
        -------
        list[TimeEvent]

        """
        cdef list events = []  # type: list[TimeEvent]
        while not self.is_expired and to_time_ns >= self.next_time_ns:
            events.append(self.pop_event(
                event_id=UUID4(),
                ts_init=self.next_time_ns,
            ))
            self.iterate_next_time(to_time_ns=self.next_time_ns)

        return events

    cpdef void cancel(self) except *:
        """
        Cancels the timer (the timer will not generate an event).
        """
        self.is_expired = True


cdef class LiveTimer(Timer):
    """
    The abstract base class for all live timers.

    Parameters
    ----------
    name : str
        The name for the timer.
    callback : Callable[[TimeEvent], None]
        The delegate to call at the next time.
    interval_ns : uint64_t
        The time interval for the timer.
    now_ns : uint64_t
        The datetime now (UTC).
    start_time_ns : uint64_t
        The start datetime for the timer (UTC).
    stop_time_ns : uint64_t, optional
        The stop datetime for the timer (UTC) (if None then timer repeats).

    Raises
    ------
    TypeError
        If `callback` is not of type `Callable`.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        str name not None,
        callback not None: Callable[[TimeEvent], None],
        uint64_t interval_ns,
        uint64_t now_ns,
        uint64_t start_time_ns,
        uint64_t stop_time_ns=0,
    ):
        Condition.valid_string(name, "name")
        super().__init__(
            name=name,
            callback=callback,
            interval_ns=interval_ns,
            start_time_ns=start_time_ns,
            stop_time_ns=stop_time_ns,
        )

        self._internal = self._start_timer(now_ns)

    cpdef void repeat(self, uint64_t now_ns) except *:
        """
        Continue the timer.

        Parameters
        ----------
        now_ns : uint64_t
            The current time to continue timing from.

        """
        self._internal = self._start_timer(now_ns)

    cpdef void cancel(self) except *:
        """
        Cancels the timer (the timer will not generate an event).
        """
        self._internal.cancel()

    cdef object _start_timer(self, uint64_t now_ns):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover


cdef class ThreadTimer(LiveTimer):
    """
    Provides a thread based timer for live trading.

    Parameters
    ----------
    name : str
        The name for the timer.
    callback : Callable[[TimeEvent], None]
        The delegate to call at the next time.
    interval_ns : uint64_t
        The time interval for the timer.
    now_ns : uint64_t
        The datetime now (UTC).
    start_time_ns : uint64_t
        The start datetime for the timer (UTC).
    stop_time_ns : uint64_t, optional
        The stop datetime for the timer (UTC) (if None then timer repeats).

    Raises
    ------
    TypeError
        If `callback` is not of type `Callable`.
    """

    def __init__(
        self,
        str name not None,
        callback not None: Callable[[TimeEvent], None],
        uint64_t interval_ns,
        uint64_t now_ns,
        uint64_t start_time_ns,
        uint64_t stop_time_ns=0,
    ):
        super().__init__(
            name=name,
            callback=callback,
            interval_ns=interval_ns,
            now_ns=now_ns,
            start_time_ns=start_time_ns,
            stop_time_ns=stop_time_ns,
        )

    cdef object _start_timer(self, uint64_t now_ns):
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

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop to run the timer on.
    name : str
        The name for the timer.
    callback : Callable[[TimeEvent], None]
        The delegate to call at the next time.
    interval_ns : uint64_t
        The time interval for the timer.
    now_ns : uint64_t
        The datetime now (UTC).
    start_time_ns : uint64_t
        The start datetime for the timer (UTC).
    stop_time_ns : uint64_t, optional
        The stop datetime for the timer (UTC) (if None then timer repeats).

    Raises
    ------
    TypeError
        If `callback` is not of type `Callable`.
    """

    def __init__(
        self,
        loop not None,
        str name not None,
        callback not None: Callable[[TimeEvent], None],
        uint64_t interval_ns,
        uint64_t now_ns,
        uint64_t start_time_ns,
        uint64_t stop_time_ns=0,
    ):
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

    cdef object _start_timer(self, uint64_t now_ns):
        return self._loop.call_later(
            nanos_to_secs(self.next_time_ns - now_ns),
            self.callback,
            self,
        )
