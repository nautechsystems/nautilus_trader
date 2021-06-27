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

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.queue cimport Queue
from nautilus_trader.common.timer cimport TimeEvent
from nautilus_trader.core.correctness cimport Condition


cdef class Throttler:
    """
    Provides a generic throttler with an internal queue.

    Will throttle messages to the given limit-interval combination.

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
            If output is not of type callable.

        """
        Condition.valid_string(name, "name")
        Condition.positive_int(limit, "limit")
        Condition.callable(output, "output")

        self._clock = clock
        self._log = LoggerAdapter(component=name, logger=logger)
        self._queue = Queue()
        self._limit = limit
        self._vouchers = limit
        self._token = name + "-REFRESH-TOKEN"
        self._interval = interval
        self._output = output

        self.name = name
        self.is_active = False
        self.is_throttling = False

    @property
    def qsize(self):
        """
        The qsize of the internal queue.

        Returns
        -------
        int

        """
        return self._queue.qsize()

    cpdef void send(self, item) except *:
        """
        Send the given item on the throttler.

        If currently idle then internal refresh token timer will start running.
        If currently throttling then item will be placed on the internal queue
        to be sent when vouchers are refreshed.

        Parameters
        ----------
        item : object
            The item to send on the throttler.

        Notes
        -----
        Test system specs: x86_64 @ 4000 MHz Linux-4.15.0-136-lowlatency-x86_64-with-glibc2.27
        Performance overhead ~0.3μs.

        """
        if not self.is_active:
            self._run_timer()
        self._queue.put_nowait(item)
        self._process_queue()

    cpdef void _process_queue(self) except *:
        while self._vouchers > 0 and not self._queue.empty():
            item = self._queue.get_nowait()
            self._output(item)
            self._vouchers -= 1

        if self._vouchers == 0 and not self._queue.empty():
            self.is_throttling = True
            self._log.debug("At limit.")
        else:
            self.is_throttling = False

    cpdef void _refresh_vouchers(self, TimeEvent event) except *:
        self._vouchers = self._limit

        if self._queue.empty():
            self.is_active = False
            self.is_throttling = False
            self._log.debug("Idle.")
        else:
            self._run_timer()
            self._process_queue()

    cdef void _run_timer(self) except *:
        self.is_active = True
        self._log.debug("Active.")
        self._clock.set_time_alert(
            name=self._token,
            alert_time=self._clock.utc_now() + self._interval,
            handler=self._refresh_vouchers,
        )
