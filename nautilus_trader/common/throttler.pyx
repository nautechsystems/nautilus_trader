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

from typing import Any, Callable, Optional

from cpython.datetime cimport timedelta
from libc.stdint cimport int64_t

from collections import deque

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.queue cimport Queue
from nautilus_trader.common.timer cimport TimeEvent
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.rust.core cimport secs_to_nanos


cdef class Throttler:
    """
    Provides a generic throttler which can either buffer or drop messages.

    Will throttle messages to the given maximum limit-interval rate.
    If an `output_drop` handler is provided, then will drop messages which
    would exceed the rate limit. Otherwise will buffer messages until within
    the rate limit, then send.

    Parameters
    ----------
    name : str
        The unique name of the throttler.
    limit : int
        The limit setting for the throttling.
    interval : timedelta
        The interval setting for the throttling.
    clock : Clock
        The clock for the throttler.
    logger : Logger
        The logger for the throttler.
    output_send : Callable[[Any], None]
        The output handler to send messages from the throttler.
    output_drop : Callable[[Any], None], optional
        The output handler to drop messages from the throttler.
        If ``None`` then messages will be buffered.

    Raises
    ------
    ValueError
        If `name` is not a valid string.
    ValueError
        If `limit` is not positive (> 0).
    ValueError
        If `interval` is not positive (> 0).
    ValueError
        If `output_send` is not of type `Callable`.
    ValueError
        If `output_drop` is not of type `Callable` or ``None``.

    Warnings
    --------
    This throttler is not thread-safe and must be called from the same thread as
    the event loop.

    The internal buffer queue is unbounded and so a bounded queue should be
    upstream.
    """

    def __init__(
        self,
        str name,
        int limit,
        timedelta interval not None,
        Clock clock not None,
        Logger logger not None,
        output_send not None: Callable[[Any], None],
        output_drop: Optional[Callable[[Any], None]] = None,
    ):
        Condition.valid_string(name, "name")
        Condition.positive_int(limit, "limit")
        Condition.positive(interval.total_seconds(), "interval.total_seconds()")
        Condition.callable(output_send, "output_send")
        Condition.callable_or_none(output_drop, "output_drop")

        self._clock = clock
        self._log = LoggerAdapter(component_name=f"Throttler-{name}", logger=logger)
        self._interval_ns = secs_to_nanos(interval.total_seconds())
        self._buffer = Queue()
        self._timer_name = f"{name}-DEQUE"
        self._timestamps = deque(maxlen=limit)
        self._output_send = output_send
        self._output_drop = output_drop
        self._warm = False  # If throttler has sent at least limit number of msgs

        self.name = name
        self.limit = limit
        self.interval = interval
        self.is_limiting = False
        self.recv_count = 0
        self.sent_count = 0

        self._log.info("READY.")

    @property
    def qsize(self):
        """
        Return the qsize of the internal buffer.

        Returns
        -------
        int

        """
        return self._buffer.qsize()

    cpdef double used(self) except *:
        """
        Return the percentage of maximum rate currently used.

        Returns
        -------
        double
            [0, 1.0].

        """
        if not self._warm:
            if self.sent_count < 2:
                return 0

        cdef int64_t spread = self._clock.timestamp_ns() - self._timestamps[-1]
        cdef int64_t diff = max_uint64(0, self._interval_ns - spread)
        cdef double used = <double>diff / <double>self._interval_ns

        if not self._warm:
            used *= <double>self.sent_count / <double>self.limit

        return used

    cpdef void send(self, msg) except *:
        """
        Send the given message through the throttler.

        Parameters
        ----------
        msg : object
            The message to send.

        """
        self.recv_count += 1

        # Throttling is active
        if self.is_limiting:
            self._limit_msg(msg)
            return

        # Check msg rate
        cdef int64_t delta_next = self._delta_next()
        if delta_next <= 0:
            self._send_msg(msg)
        else:
            # Start throttling
            self._limit_msg(msg)

    cdef int64_t _delta_next(self) except *:
        if not self._warm:
            if self.sent_count < self.limit:
                return 0
            self._warm = True

        cdef int64_t diff = self._timestamps[0] - self._timestamps[-1]
        return self._interval_ns - diff

    cdef void _limit_msg(self, msg) except *:
        if self._output_drop is None:
            # Buffer
            self._buffer.put_nowait(msg)
            timer_target = self._process
            self._log.warning(f"Buffering {msg}.")
        else:
            # Drop
            self._output_drop(msg)
            timer_target = self._resume
            self._log.warning(f"Dropped {msg}.")

        if not self.is_limiting:
            self._set_timer(timer_target)
            self.is_limiting = True

    cdef void _set_timer(self, handler: Callable[[TimeEvent], None]) except *:
        self._clock.set_time_alert_ns(
            name=self._timer_name,
            alert_time_ns=self._clock.timestamp_ns() + self._delta_next(),
            callback=handler,
        )

    cpdef void _process(self, TimeEvent event) except *:
        # Send next msg on buffer
        msg = self._buffer.get_nowait()
        self._send_msg(msg)

        # Send remaining messages if within rate
        cdef int64_t delta_next
        while not self._buffer.empty():
            delta_next = self._delta_next()
            if delta_next <= 0:
                self._send_msg(msg)
            else:
                self._set_timer(self._process)
                return

        # No longer throttling
        self.is_limiting = False

    cpdef void _resume(self, TimeEvent event) except *:
        self.is_limiting = False

    cdef void _send_msg(self, msg) except *:
        self._timestamps.appendleft(self._clock.timestamp_ns())
        self._output_send(msg)
        self.sent_count += 1


cdef inline uint64_t max_uint64(uint64_t a, uint64_t b):
    if a > b:
        return a
    else:
        return b
