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

import logging
import os
import platform
from platform import python_version
import sys
import threading
import traceback

import numpy as np
import pandas as pd
import psutil
import scipy

from nautilus_trader import __version__

from cpython.datetime cimport datetime

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.log_queue cimport LogQueue
from nautilus_trader.common.logging cimport LogLevel
from nautilus_trader.common.logging cimport LogMessage
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport format_iso8601


cdef str _HEADER = "\033[95m"
cdef str _OK_BLUE = "\033[94m"
cdef str _OK_GREEN = "\033[92m"
cdef str _WARN = "\033[1;33m"
cdef str _FAIL = "\033[01;31m"
cdef str _ENDC = "\033[0m"
cdef str _BOLD = "\033[1m"
cdef str _UNDERLINE = "\033[4m"

RECV = "<--"
SENT = "-->"
CMD = "[CMD]"
EVT = "[EVT]"
REQ = "[REQ]"
RES = "[RES]"


cdef class LogLevelParser:

    @staticmethod
    cdef str to_str(int value):
        if value == 1:
            return "VRB"
        elif value == 2:
            return "DBG"
        elif value == 3:
            return "INF"
        elif value == 4:
            return "WRN"
        elif value == 5:
            return "ERR"
        elif value == 6:
            return "CRT"
        elif value == 7:
            return "FTL"
        else:
            return "UNDEFINED"

    @staticmethod
    cdef LogLevel from_str(str value):
        if value == "VRB":
            return LogLevel.VERBOSE
        elif value == "DBG":
            return LogLevel.DEBUG
        elif value == "INF":
            return LogLevel.INFO
        elif value == "WRN":
            return LogLevel.WARNING
        elif value == "ERR":
            return LogLevel.ERROR
        elif value == "CRT":
            return LogLevel.CRITICAL
        elif value == "FTL":
            return LogLevel.FATAL
        else:
            return LogLevel.UNDEFINED

    @staticmethod
    def to_str_py(int value):
        return LogLevelParser.to_str(value)

    @staticmethod
    def from_str_py(str value):
        return LogLevelParser.from_str(value)


cdef class LogMessage:
    """
    Represents a log message including timestamp and log level.
    """
    def __init__(
        self,
        datetime timestamp not None,
        LogLevel level,
        str text not None,
        long thread_id=0,
    ):
        """
        Initialize a new instance of the `LogMessage` class.

        Parameters
        ----------
        timestamp : datetime
            The log message timestamp.
        level :  LogLevel
            The log message level.
        text : str
            The log message text.
        thread_id : long, optional
            The thread the log message was created on.

        """
        self.timestamp = timestamp
        self.level = level
        self.text = text
        self.thread_id = thread_id

    cdef str level_string(self):
        """
        Return the string representation of the log level.

        Returns
        -------
        str

        """
        return LogLevelParser.to_str(self.level)

    cdef str as_string(self):
        """
        Return the string representation of the log message.

        Returns
        -------
        str

        """
        return f"{format_iso8601(self.timestamp)} [{self.thread_id}][{LogLevelParser.to_str(self.level)}] {self.text}"


cdef class Logger:
    """
    The abstract base class for all Loggers.

    This class should not be used directly, but through its concrete subclasses.
    """

    def __init__(
        self,
        Clock clock not None,
        str name=None,
        bint bypass_logging=False,
        LogLevel level_console=LogLevel.INFO,
        LogLevel level_file=LogLevel.DEBUG,
        LogLevel level_store=LogLevel.WARNING,
        bint console_prints=True,
        bint log_thread=False,
        bint log_to_file=False,
        str log_file_path not None="",
    ):
        """
        Initialize a new instance of the `Logger` class.

        Parameters
        ----------
        clock : Clock
            The clock for the logger.
        name : str
            The name of the logger.
        bypass_logging : bool
            If the logger should be completely bypassed.
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
            If log messages should be written to the log file.
        log_file_path : str
            The name of the log file (cannot be None if log_to_file is True).

        Raises
        ------
        ValueError
            If name is not a valid string.
        ValueError
            If log_file_path is not a valid string.

        """
        if name is not None:
            Condition.valid_string(name, "name")
        else:
            name = "tmp"
        if log_to_file:
            if log_file_path == "":
                log_file_path = "log/"
            Condition.valid_string(log_file_path, "log_file_path")

        self.name = name
        self.bypass_logging = bypass_logging
        self.clock = clock
        self._log_level_console = level_console
        self._log_level_file = level_file
        self._log_level_store = level_store
        self._console_prints = console_prints
        self._log_thread = log_thread
        self._log_to_file = log_to_file
        self._log_file_path = log_file_path
        self._log_file = f"{self._log_file_path}{self.name}-{self.clock.utc_now().date().isoformat()}.log"
        self._log_store = []
        self._logger = logging.getLogger(name)
        self._logger.setLevel(logging.DEBUG)

        # Setup log file handling
        if log_to_file:
            if not os.path.exists(log_file_path):
                # Create directory if it does not exist
                os.makedirs(log_file_path)
            self._log_file_handler = logging.FileHandler(self._log_file)
            self._logger.addHandler(self._log_file_handler)

    cpdef void change_log_file_name(self, str name) except *:
        """
        Change the log file name.

        Parameters
        ----------
        name : str
            The new name of the log file.

        """
        Condition.valid_string(name, "name")

        self._log_file = f"{self._log_file_path}{name}.log"
        self._logger.removeHandler(self._log_file_handler)
        self._log_file_handler = logging.FileHandler(self._log_file)
        self._logger.addHandler(self._log_file_handler)

    cpdef void log(self, LogMessage message) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef list get_log_store(self):
        """
        Return the log store of message strings.

        Returns
        -------
        list[str]

        """
        return self._log_store

    cpdef void clear_log_store(self) except *:
        """
        Clear the log store.
        """
        self._log_store = []

    cpdef void _log(self, LogMessage message) except *:
        cdef str formatted_msg = self._format_output(message)
        self._in_memory_log_store(message.level, formatted_msg)
        self._print_to_console(message.level, formatted_msg)

        if self._log_to_file and message.level >= self._log_level_file:
            try:
                self._logger.debug(message.as_string())
            except IOError as ex:
                self._print_to_console(LogLevel.ERROR, f"IOError: {ex}.")

    cdef str _format_output(self, LogMessage message):
        # Return the formatted log message from the given arguments
        cdef str time = format_iso8601(message.timestamp)
        cdef str thread = "" if self._log_thread is False else f"[{message.thread_id}]"
        cdef str formatted_text

        if message.level == LogLevel.WARNING:
            formatted_text = f"{_WARN}[{message.level_string()}] {message.text}{_ENDC}"
        elif message.level == LogLevel.ERROR:
            formatted_text = f"{_FAIL}[{message.level_string()}] {message.text}{_ENDC}"
        elif message.level == LogLevel.CRITICAL:
            formatted_text = f"{_FAIL}[{message.level_string()}] {message.text}{_ENDC}"
        else:
            formatted_text = f"[{message.level_string()}] {message.text}"

        return f"{_BOLD}{time}{_ENDC} {thread}{formatted_text}"

    cdef void _in_memory_log_store(self, LogLevel level, str text) except *:
        # Store the given log message if the given log level is >= the log_level_store
        if level >= self._log_level_store:
            self._log_store.append(text)

    cdef void _print_to_console(self, LogLevel level, str text) except *:
        # Print the given log message to the console if the given log level if
        # >= the log_level_console level.
        if self._console_prints and level >= self._log_level_console:
            print(text)


cdef class LoggerAdapter:
    """
    Provides an adapter for a components logger.
    """

    def __init__(
        self,
        str component_name not None,
        Logger logger not None,
    ):
        """
        Initialize a new instance of the `LoggerAdapter` class.

        Parameters
        ----------
        component_name : str
            The name of the component.
        logger : Logger
            The logger for the component.

        """
        Condition.valid_string(component_name, "component_name")

        self._logger = logger
        self.component_name = component_name
        self.bypassed = logger.bypass_logging

    cpdef Logger get_logger(self):
        """
        Return the encapsulated logger

        Returns
        -------
        Logger

        """
        return self._logger

    cpdef void verbose(self, str message) except *:
        """
        Log the given verbose message with the logger.

        Parameters
        ----------
        message : str
            The message to log.

        """
        Condition.not_none(message, "message")

        self._send_to_logger(LogLevel.VERBOSE, message)

    cpdef void debug(self, str message) except *:
        """
        Log the given debug message with the logger.

        Parameters
        ----------
        message : str
            The message to log.

        """
        Condition.not_none(message, "message")

        self._send_to_logger(LogLevel.DEBUG, message)

    cpdef void info(self, str message) except *:
        """
        Log the given information message with the logger.

        Parameters
        ----------
        message : str
            The message to log.

        """
        Condition.not_none(message, "message")

        self._send_to_logger(LogLevel.INFO, message)

    cpdef void warning(self, str message) except *:
        """
        Log the given warning message with the logger.

        Parameters
        ----------
        message : str
            The message to log.

        """
        Condition.not_none(message, "message")

        self._send_to_logger(LogLevel.WARNING, message)

    cpdef void error(self, str message) except *:
        """
        Log the given error message with the logger.

        Parameters
        ----------
        message : str
            The message to log.

        """
        Condition.not_none(message, "message")

        self._send_to_logger(LogLevel.ERROR, message)

    cpdef void critical(self, str message) except *:
        """
        Log the given critical message with the logger.

        Parameters
        ----------
        message : str
            The message to log.

        """
        Condition.not_none(message, "message")

        self._send_to_logger(LogLevel.CRITICAL, message)

    cpdef void exception(self, ex) except *:
        """
        Log the given exception including stack trace information.

        Parameters
        ----------
        ex : Exception
            The message to log.

        """
        Condition.not_none(ex, "ex")

        cdef str ex_string = f"{type(ex).__name__}({ex})\n"
        exc_type, exc_value, exc_traceback = sys.exc_info()
        stack_trace = traceback.format_exception(exc_type, exc_value, exc_traceback)

        cdef str stack_trace_lines = ""
        cdef str line
        for line in stack_trace[:len(stack_trace) - 1]:
            stack_trace_lines += line

        self.error(f"{ex_string}{ stack_trace_lines}")

    cdef inline void _send_to_logger(self, LogLevel level, str message) except *:
        if not self.bypassed:
            self._logger.log(LogMessage(
                self._logger.clock.utc_now(),
                level,
                self._format_message(message),
                thread_id=threading.current_thread().ident),
            )

    cdef inline str _format_message(self, str message):
        # Add the components name to the front of the log message
        return f"{self.component_name}: {message}"


cpdef void nautilus_header(LoggerAdapter logger) except *:
    Condition.not_none(logger, "logger")
    print("")  # New line to begin
    logger.info("=================================================================")
    logger.info(f" NAUTILUS TRADER - Algorithmic Trading Platform")
    logger.info(f" by Nautech Systems Pty Ltd.")
    logger.info(f" Copyright (C) 2015-2020. All rights reserved.")
    logger.info("=================================================================")
    logger.info("                                                                 ")
    logger.info("                            .......                              ")
    logger.info("                         .............                           ")
    logger.info("    .                  ......... .......                         ")
    logger.info("   .                  ......... .. .......                       ")
    logger.info("   .                 ......',,,,'..........                      ")
    logger.info("   ..               ......::,,''';,.........                     ")
    logger.info("   ..                ....'o:;oo;..:'..... ''                     ")
    logger.info("    ..               ......,;,,..,:'.........                    ")
    logger.info("    ..                .........';:'..... ...                     ")
    logger.info("     ..                 .......'..... .'. .'                     ")
    logger.info("      ..                   .....    .. .. ..                     ")
    logger.info("       ..                           .' ....                      ")
    logger.info("         ..                         .. .'.                       ")
    logger.info("          ....                     .....                         ")
    logger.info("             ....                ..'..                           ")
    logger.info("                 ..................                              ")
    logger.info("                                                                 ")
    logger.info("=================================================================")
    logger.info(" SYSTEM SPECIFICATION")
    logger.info("=================================================================")
    logger.info(f"CPU architecture: {platform.processor()}")
    cpu_freq_str = "" if psutil.cpu_freq() is None else f"@ {int(psutil.cpu_freq()[2])} MHz"
    logger.info(f"CPU(s): {psutil.cpu_count()} {cpu_freq_str}")
    ram_total_mb = round(psutil.virtual_memory()[0] / 1000000)
    ram_used__mb = round(psutil.virtual_memory()[3] / 1000000)
    ram_avail_mb = round(psutil.virtual_memory()[1] / 1000000)
    ram_avail_pc = round(100 - psutil.virtual_memory()[2], 2)
    logger.info(f"RAM-Total: {ram_total_mb:,} MB")
    logger.info(f"RAM-Used:  {ram_used__mb:,} MB ({round(100.0 - ram_avail_pc, 2)}%)")
    logger.info(f"RAM-Avail: {ram_avail_mb:,} MB ({ram_avail_pc}%)")
    logger.info(f"OS: {platform.platform()}")
    logger.info("=================================================================")
    logger.info(" VERSIONING")
    logger.info("=================================================================")
    logger.info(f"nautilus-trader {__version__}")
    logger.info(f"python {python_version()}")
    logger.info(f"numpy {np.__version__}")
    logger.info(f"scipy {scipy.__version__}")
    logger.info(f"pandas {pd.__version__}")


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
    ):
        """
        Initialize a new instance of the `LiveLogger` class.

        Parameters
        ----------
        clock : LiveClock
            The clock for the logger.
        name : str
            The name of the logger.
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
            If the name is not a valid string.
        ValueError
            If the log_file_path is not a valid string.

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

        self._queue = LogQueue()
        self._thread = threading.Thread(target=self._consume_messages, daemon=True)
        self._thread.start()

    cpdef void log(self, LogMessage message) except *:
        """
        Log the given message.

        Parameters
        ----------
        message : LogMessage
            The log message to log.

        """
        Condition.not_none(message, "message")

        self._queue.put(message)

    cpdef void _consume_messages(self) except *:
        cdef LogMessage message
        while True:
            message = self._queue.get()
            self._log(message)
