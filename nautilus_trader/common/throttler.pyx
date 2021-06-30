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

from cpython.datetime cimport timedelta
from libc.stdint cimport int64_t

from collections import deque

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.queue cimport Queue
from nautilus_trader.common.timer cimport TimeEvent
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport nanos_to_unix_dt
from nautilus_trader.core.datetime cimport secs_to_nanos


cdef class Throttler:
    """
    Provides a generic throttler with an internal queue.

    Will throttle messages to the given maximum limit-interval rate.

    Warnings
    --------
    This throttler is not thread-safe and must be called from the same thread as
    the event loop.

    The internal queue is unbounded and so a bounded queue should be upstream.
    """

    def __init__(
        self,
        str name,
        int limit,
        timedelta interval not None,
        output not None: callable,
        Clock clock not None,
        Logger logger not None,
    ):
        """
        Initialize a new instance of the ``Throttler`` class.

        Parameters
        ----------
        name : str
            The unique name of the throttler.
        limit : int
            The limit setting for the throttling.
        interval : timedelta
            The interval setting for the throttling.
        output : callable
            The output handler from the throttler.
        clock : Clock
            The clock for the throttler.
        logger : Logger
            The logger for the throttler.

        Raises
        ------
        ValueError
            If name is not a valid string.
        ValueError
            If limit is not positive (> 0).
        ValueError
            If interval is not positive (> 0).
        ValueError
            If output is not of type callable.

        """
        Condition.valid_string(name, "name")
        Condition.positive_int(limit, "limit")
        Condition.positive(interval.total_seconds(), "interval.total_seconds()")
        Condition.callable(output, "output")

        self._clock = clock
        self._log = LoggerAdapter(component=name, logger=logger)
        self._limit = limit
        self._interval = interval
        self._interval_ns = secs_to_nanos(interval.total_seconds())
        self._buffer = Queue()
        self._timestamps = deque(maxlen=limit)
        self._timer_name = f"{name}-DEQUE"
        self._output = output
        self._initialized = False

        self.name = name
        self.is_buffering = False

    @property
    def qsize(self):
        """
        The qsize of the internal buffer.

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
            [0, 1.0]

        """
        return max(0, self._delta_next()) / self._interval_ns

    cpdef void send(self, item) except *:
        """
        Send the given item through the throttler.

        Parameters
        ----------
        item : object
            The item to send.

        """
        # Throttling is occurring: place item on buffer
        if self.is_buffering:
            self._buffer.put_nowait(item)
            return

        cdef int64_t delta_next = self._delta_next()
        if delta_next <= 0:
            self._send_item(item)
            return

        # Start throttling
        self.is_buffering = True
        self._buffer.put_nowait(item)
        self._set_timer(delta_next)

    cdef int64_t _delta_next(self) except *:
        if not self._initialized:
            if len(self._timestamps) < self._limit:
                return 0
            else:
                self._initialized = True

        cdef int64_t diff = self._timestamps[0] - self._timestamps[-1]
        return self._interval_ns - diff

    cpdef void _process(self, TimeEvent event) except *:
        item = self._buffer.get_nowait()
        self._send_item(item)

        cdef int64_t delta_next
        while not self._buffer.empty():
            delta_next = self._delta_next()
            if delta_next <= 0:
                self._send_item(item)
                continue

            self._set_timer(delta_next)
            break

        self.is_buffering = False

    cdef void _set_timer(self, int64_t delta_next) except *:
        self._clock.set_time_alert(
            name=self._timer_name,
            alert_time=nanos_to_unix_dt(self._clock.timestamp_ns() + delta_next),
            handler=self._process,
        )

    cdef void _send_item(self, item) except *:
        self._timestamps.appendleft(self._clock.timestamp_ns())
        self._output(item)
