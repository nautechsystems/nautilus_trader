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

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport LogLevel
from nautilus_trader.common.logging cimport LogMessage
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.correctness cimport Condition


cdef class TestLogger(Logger):
    """
    Provides a single threaded logger for testing.
    """
    __test__ = False

    def __init__(
            self,
            Clock clock not None,
            str name=None,
            bint bypass_logging=False,
            LogLevel level_console=LogLevel.DEBUG,
            LogLevel level_file=LogLevel.DEBUG,
            LogLevel level_store=LogLevel.WARNING,
            bint console_prints=True,
            bint log_thread=False,
            bint log_to_file=False,
            str log_file_path not None="log/",
    ):
        """
        Initialize a new instance of the TestLogger class.

        :param clock: The clock for the logger.
        :param name: The name of the logger.
        :param level_console: The minimum log level for logging messages to the console.
        :param level_file: The minimum log level for logging messages to the log file.
        :param level_store: The minimum log level for storing log messages in memory.
        :param console_prints: If log messages should print to the console.
        :param log_thread: If log messages should include the thread.
        :param log_to_file: If log messages should write to the log file.
        :param log_file_path: The name of the log file (cannot be None if log_to_file is True).
        :raises ValueError: If name is not a valid string.
        :raises ValueError: If log_file_path is not a valid string.
        """
        if log_file_path is "":
            log_file_path = "log/"
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

    cpdef void log(self, LogMessage message) except *:
        """
        Log the given log message.

        :param message: The log message to log.
        """
        Condition.not_none(message, "message")

        self._log(message)
