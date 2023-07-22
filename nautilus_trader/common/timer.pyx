# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from threading import Timer as TimerThread
from typing import Callable

from libc.stdint cimport uint64_t

from nautilus_trader.common.timer cimport TimeEvent
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.message cimport Event
from nautilus_trader.core.rust.common cimport time_event_clone
from nautilus_trader.core.rust.common cimport time_event_drop
from nautilus_trader.core.rust.common cimport time_event_name_to_cstr
from nautilus_trader.core.rust.common cimport time_event_new
from nautilus_trader.core.rust.common cimport time_event_to_cstr
from nautilus_trader.core.rust.core cimport nanos_to_secs
from nautilus_trader.core.rust.core cimport uuid4_from_cstr
from nautilus_trader.core.string cimport cstr_to_pystr
from nautilus_trader.core.string cimport pystr_to_cstr
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
        # Precondition: `name` validated in Rust
        super().__init__(event_id, ts_event, ts_init)

        self._mem = time_event_new(
            pystr_to_cstr(name),
            event_id._mem,
            ts_event,
            ts_init,
        )

    def __del__(self) -> None:
        if self._mem.name != NULL:
            time_event_drop(self._mem)  # `self._mem` moved to Rust (then dropped)

    def __getstate__(self):
        return (
            self.to_str(),
            self.id.to_str(),
            self.ts_event,
            self.ts_init,
        )

    def __setstate__(self, state):
        self.ts_event = state[2]
        self.ts_init = state[3]
        self._mem = time_event_new(
            pystr_to_cstr(state[0]),
            uuid4_from_cstr(pystr_to_cstr(state[1])),
            self.ts_event,
            self.ts_init,
        )

    cdef str to_str(self):
        return cstr_to_pystr(time_event_name_to_cstr(&self._mem))

    def __eq__(self, TimeEvent other) -> bool:
        return self.to_str() == other.to_str()

    def __hash__(self) -> int:
        return hash(self.to_str())

    def __str__(self) -> str:
        return self.to_str()

    def __repr__(self) -> str:
        return cstr_to_pystr(time_event_to_cstr(&self._mem))

    @property
    def name(self) -> str:
        """
        Return the name of the time event.

        Returns
        -------
        str

        """
        return cstr_to_pystr(time_event_name_to_cstr((&self._mem)))

    @staticmethod
    cdef TimeEvent from_mem_c(TimeEvent_t mem):
        cdef TimeEvent event = TimeEvent.__new__(TimeEvent)
        event._mem = time_event_clone(&mem)
        event.id = UUID4.from_mem_c(mem.event_id)
        event.ts_event = mem.ts_event
        event.ts_init = mem.ts_init
        return event


cdef class TimeEventHandler:
    """
    Represents a time event with its associated handler.
    """

    def __init__(
        self,
        TimeEvent event not None,
        handler not None: Callable[[TimeEvent], None],
    ):
        self.event = event
        self._handler = handler

    cpdef void handle(self):
        """Call the handler with the contained time event."""
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
            f"event={repr(self.event)})"
        )


cdef class LiveTimer:
    """
    The base class for all live timers.

    Parameters
    ----------
    name : str
        The name for the timer.
    callback : Callable[[TimeEvent], None]
        The delegate to call at the next time.
    interval_ns : uint64_t
        The time interval for the timer.
    ts_now : uint64_t
        The current UNIX time (nanoseconds).
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
        uint64_t ts_now,
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

        self._internal = self._start_timer(ts_now)

    def __eq__(self, LiveTimer other) -> bool:
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
        # Precondition: `event_id` validated in `TimeEvent`

        return TimeEvent(
            name=self.name,
            event_id=event_id,
            ts_event=self.next_time_ns,
            ts_init=ts_init,
        )

    cpdef void iterate_next_time(self, uint64_t ts_now):
        """
        Iterates the timers next time and checks if the timer is now expired.

        Parameters
        ----------
        ts_now : uint64_t
            The current UNIX time (nanoseconds).

        """
        self.next_time_ns += self.interval_ns
        if self.stop_time_ns and ts_now >= self.stop_time_ns:
            self.is_expired = True

    cpdef void repeat(self, uint64_t ts_now):
        """
        Continue the timer.

        Parameters
        ----------
        ts_now : uint64_t
            The current time to continue timing from.

        """
        self._internal = self._start_timer(ts_now)

    cpdef void cancel(self):
        """
        Cancels the timer (the timer will not generate an event).
        """
        self._internal.cancel()

    cdef object _start_timer(self, uint64_t ts_now):
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
    ts_now : uint64_t
        The current UNIX time (nanoseconds).
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
        uint64_t ts_now,
        uint64_t start_time_ns,
        uint64_t stop_time_ns=0,
    ):
        super().__init__(
            name=name,
            callback=callback,
            interval_ns=interval_ns,
            ts_now=ts_now,
            start_time_ns=start_time_ns,
            stop_time_ns=stop_time_ns,
        )

    cdef object _start_timer(self, uint64_t ts_now):
        timer = TimerThread(
            interval=nanos_to_secs(self.next_time_ns - ts_now),
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
        The time interval for the timer (nanoseconds).
    ts_now : uint64_t
        The current UNIX epoch (nanoseconds).
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
        uint64_t ts_now,
        uint64_t start_time_ns,
        uint64_t stop_time_ns=0,
    ):
        Condition.valid_string(name, "name")

        self._loop = loop  # Assign here as `super().__init__` will call it
        super().__init__(
            name=name,
            callback=callback,
            interval_ns=interval_ns,
            ts_now=ts_now,
            start_time_ns=start_time_ns,
            stop_time_ns=stop_time_ns,
        )

    cdef object _start_timer(self, uint64_t ts_now):
        return self._loop.call_later(
            nanos_to_secs(self.next_time_ns - ts_now),
            self.callback,
            self,
        )
