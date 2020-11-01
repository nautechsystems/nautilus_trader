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
        Initialize a new instance of the `TestLogger` class.

        Parameters
        ----------
        clock : Clock
            The clock for the logger.
        name : str
            The name of the logger.
        bypass_logging : bool
            If logging should be entirely bypasses.
        level_console : LogLevel
            The minimum log level for logging messages to the console.
        level_file : LogLevel
            The minimum log level for logging messages to the log file.
        level_store : LogLevel
            The minimum log level for storing log messages in memory.
        console_prints : bool
            If log messages should print to the console.
        log_thread : bool
            If log messages should include the thread.
        log_to_file : bool
            If log messages should write to the log file.
        log_file_path : str
            The name of the log file (cannot be None if log_to_file is True).

        Raises
        ------
        ValueError
            If name is not a valid string.
        ValueError
            If log_file_path is not a valid string.

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

        Parameters
        ----------
        message : LogMessage
            The message to log.

        """
        Condition.not_none(message, "message")

        self._log(message)
