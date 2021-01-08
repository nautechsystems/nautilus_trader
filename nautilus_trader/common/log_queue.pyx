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

import threading
from collections import deque

from nautilus_trader.common.logging import LogMessage


cdef class LogQueue:
    """
    Provides a high performance log message queue.
    """

    def __init__(self):
        self._internal = deque()
        self._mutex = threading.Lock()
        self._not_empty = threading.Condition(self._mutex)

    cpdef void put(self, LogMessage message) except *:
        """
        Put a log message on the queue.

        Parameters
        ----------
        message : LogMessage
            The log message.

        """
        with self._not_empty:
            self._internal.append(message)
            self._not_empty.notify()

    cpdef LogMessage get(self):
        """
        Remove a log message from the queue when available.

        Returns
        -------
        LogMessage

        """
        cdef LogMessage message
        with self._not_empty:
            # Wait for next log message
            while len(self._internal) == 0:
                self._not_empty.wait()

            message = self._internal.popleft()
            return message
