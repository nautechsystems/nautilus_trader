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

import queue
import threading

from nautilus_trader.common.logging cimport LogLevel
from nautilus_trader.common.logging cimport LogMessage
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.live.clock cimport LiveClock
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.serialization.base cimport LogSerializer
from nautilus_trader.serialization.serializers cimport MsgPackLogSerializer


cdef class LogStore:
    """
    Provides a process and thread safe log store.
    """

    def __init__(
            self,
            TraderId trader_id not None,
            LogSerializer serializer not None=MsgPackLogSerializer(),
    ):
        """
        Initialize a new instance of the LogStore class.

        :param trader_id: The trader_id.
        :raises ValueError: If the redis_host is not a valid string.
        :raises ValueError: If the redis_port is not in range [0, 65535].
        """
        self._key = f"Trader-{trader_id.value}:LogStore"
        self._serializer = serializer

    cpdef void store(self, LogMessage message):
        """
        Store the given log message.

        :param message: The log message to store.
        """
        Condition.not_none(message, "message")

        # self._queue.put(message)

    cpdef void _consume_messages(self) except *:
        cdef LogMessage message
        while True:
            pass
            # Scaffolding for a future LogStash implementation


cdef class LiveLogger(Logger):
    """
    Provides a thread safe logger for live concurrent operations.
    """

    def __init__(
            self,
            LiveClock clock not None,
            str name=None,
            bint bypass_logging=False,
            LogLevel level_console=LogLevel.INFO,
            LogLevel level_file=LogLevel.DEBUG,
            LogLevel level_store=LogLevel.WARNING,
            bint console_prints=True,
            bint log_thread=False,
            bint log_to_file=False,
            str log_file_path not None="logs/",
            LogStore store=None,
    ):
        """
        Initialize a new instance of the LiveLogger class.

        :param clock: The clock for the logger.
        :param name: The name of the logger.
        :param level_console: The minimum log level for logging messages to the console.
        :param level_file: The minimum log level for logging messages to the log file.
        :param level_store: The minimum log level for storing log messages in memory.
        :param console_prints: If log messages should print to the console.
        :param log_thread: If log messages should include the thread.
        :param log_to_file: If log messages should write to the log file.
        :param log_file_path: The name of the log file (cannot be None if log_to_file is True).
        :raises ValueError: If the name is not a valid string.
        :raises ValueError: If the log_file_path is not a valid string.
        """
        super().__init__(
            clock,
            name,
            bypass_logging,
            level_console,
            level_file,
            level_store,
            console_prints,
            log_thread,
            log_to_file,
            log_file_path,
        )

        self._store = store
        self._queue = queue.Queue()
        self._thread = threading.Thread(target=self._consume_messages, daemon=True)
        self._thread.start()

    cpdef void log(self, LogMessage message) except *:
        """
        Log the given message.

        :param message: The log message to log.
        """
        Condition.not_none(message, "message")

        self._queue.put(message)

    cpdef void _consume_messages(self) except *:
        cdef LogMessage message
        while True:
            message = self._queue.get()
            self._log(message)
