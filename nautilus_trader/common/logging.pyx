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
import multiprocessing
import os
import platform
from platform import python_version
import queue
import sys
import threading
import traceback

import numpy as np
import pandas as pd
import psutil
import scipy

from nautilus_trader import __version__

from cpython.datetime cimport datetime
from libc.stdint cimport int64_t

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logging cimport LogLevel
from nautilus_trader.common.logging cimport LogMessage
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport format_iso8601_us


cdef str _HEADER = "\033[95m"
cdef str _BLUE = "\033[94m"
cdef str _GREEN = "\033[92m"
cdef str _YELLOW = "\033[1;33m"
cdef str _RED = "\033[01;31m"
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
        LogColor color,
        str text not None,
        int64_t thread_id=0,
    ):
        """
        Initialize a new instance of the `LogMessage` class.

        Parameters
        ----------
        timestamp : datetime
            The log message timestamp.
        level :  LogLevel (Enum)
            The log message level.
        color :  LogColor (Enum)
            The log message color.
        text : str
            The log message text.
        thread_id : int64, optional
            The thread the log message was created on.

        """
        self.timestamp = timestamp
        self.level = level
        self.color = color
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
        return f"{format_iso8601_us(self.timestamp)} [{self.thread_id}][{LogLevelParser.to_str(self.level)}] {self.text}"


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
            If the logger should be bypassed.
        level_console : LogLevel (Enum)
            The minimum log level for logging messages to the console.
        level_file : LogLevel (Enum)
            The minimum log level for logging messages to the log file.
        level_store : LogLevel (Enum)
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
        cdef str time = format_iso8601_us(message.timestamp)
        cdef str thread = "" if self._log_thread is False else f"[{message.thread_id}]"
        cdef str colour_cmd

        if message.color == LogColor.NORMAL:
            colour_cmd = ""
        elif message.color == LogColor.BLUE:
            colour_cmd = _BLUE
        elif message.color == LogColor.GREEN:
            colour_cmd = _GREEN
        elif message.color == LogColor.YELLOW:
            colour_cmd = _YELLOW
        elif message.color == LogColor.RED:
            colour_cmd = _RED
        else:
            colour_cmd = ""

        return (f"{_BOLD}{time}{_ENDC} {thread}{colour_cmd}"
                f"[{LogLevelParser.to_str(message.level)}] {message.text}{_ENDC}")

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
        self.is_bypassed = logger.bypass_logging

    cpdef Logger get_logger(self):
        """
        Return the encapsulated logger

        Returns
        -------
        Logger

        """
        return self._logger

    cpdef void verbose(self, str message, LogColor color=LogColor.NORMAL) except *:
        """
        Log the given verbose message with the logger.

        Parameters
        ----------
        message : str
            The message to log.
        color : LogColor (Enum), optional
            The text color for the message.

        """
        Condition.not_none(message, "message")

        self._send_to_logger(LogLevel.VERBOSE, color, message)

    cpdef void debug(self, str message, LogColor color=LogColor.NORMAL) except *:
        """
        Log the given debug message with the logger.

        Parameters
        ----------
        message : str
            The message to log.
        color : LogColor (Enum), optional
            The text color for the message.

        """
        Condition.not_none(message, "message")

        self._send_to_logger(LogLevel.DEBUG, color, message)

    cpdef void info(self, str message, LogColor color=LogColor.NORMAL) except *:
        """
        Log the given information message with the logger.

        Parameters
        ----------
        message : str
            The message to log.
        color : LogColor (Enum), optional
            The text color for the message.

        """
        Condition.not_none(message, "message")

        self._send_to_logger(LogLevel.INFO, color, message)

    cpdef void warning(self, str message) except *:
        """
        Log the given warning message with the logger.

        Parameters
        ----------
        message : str
            The message to log.

        """
        Condition.not_none(message, "message")

        self._send_to_logger(LogLevel.WARNING, LogColor.YELLOW, message)

    cpdef void error(self, str message) except *:
        """
        Log the given error message with the logger.

        Parameters
        ----------
        message : str
            The message to log.

        """
        Condition.not_none(message, "message")

        self._send_to_logger(LogLevel.ERROR, LogColor.RED, message)

    cpdef void critical(self, str message) except *:
        """
        Log the given critical message with the logger.

        Parameters
        ----------
        message : str
            The message to log.

        """
        Condition.not_none(message, "message")

        self._send_to_logger(LogLevel.CRITICAL, LogColor.RED, message)

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
        ex_type, ex_value, ex_traceback = sys.exc_info()
        stack_trace = traceback.format_exception(ex_type, ex_value, ex_traceback)

        cdef str stack_trace_lines = ""
        cdef str line
        for line in stack_trace[:len(stack_trace) - 1]:
            stack_trace_lines += line

        self.error(f"{ex_string} {stack_trace_lines}")

    cdef inline void _send_to_logger(
        self,
        LogLevel level,
        LogColor color,
        str message,
    ) except *:
        if not self.is_bypassed:
            self._logger.log(LogMessage(
                self._logger.clock.utc_now(),
                level,
                color,
                self._format_message(message),
                threading.current_thread().ident),
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
    logger.info(f" Copyright (C) 2015-2021. All rights reserved.")
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
    try:
        cpu_freq_str = f"@ {int(psutil.cpu_freq()[2])} MHz"
    except NotImplementedError:
        cpu_freq_str = None
    logger.info(f"CPU(s): {psutil.cpu_count()} {cpu_freq_str}")
    logger.info(f"OS: {platform.platform()}")
    log_memory(logger)
    logger.info("=================================================================")
    logger.info(" VERSIONING")
    logger.info("=================================================================")
    logger.info(f"nautilus-trader {__version__}")
    logger.info(f"python {python_version()}")
    logger.info(f"numpy {np.__version__}")
    logger.info(f"scipy {scipy.__version__}")
    logger.info(f"pandas {pd.__version__}")


cpdef void log_memory(LoggerAdapter logger) except *:
    logger.info("=================================================================")
    logger.info(" MEMORY USAGE")
    logger.info("=================================================================")
    ram_total_mb = round(psutil.virtual_memory()[0] / 1000000)
    ram_used__mb = round(psutil.virtual_memory()[3] / 1000000)
    ram_avail_mb = round(psutil.virtual_memory()[1] / 1000000)
    ram_avail_pc = 100 - psutil.virtual_memory()[2]
    ram_avail_colour = LogColor.NORMAL if ram_avail_pc > 50 else LogColor.YELLOW
    logger.info(f"RAM-Total: {ram_total_mb:,} MB")
    logger.info(f"RAM-Used:  {ram_used__mb:,} MB ({100 - ram_avail_pc:.2f}%)")
    logger.info(f"RAM-Avail: {ram_avail_mb:,} MB ({ram_avail_pc:.2f}%)", ram_avail_colour)


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
        level_console : LogLevel (Enum)
            The minimum log level for logging messages to the console.
        level_file : LogLevel (Enum)
            The minimum log level for logging messages to the log file.
        level_store : LogLevel (Enum)
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
    Provides a high-performance logger which runs in a separate process for live
    operations.
    """

    def __init__(
        self,
        LiveClock clock not None,
        str name=None,
        bint bypass_logging=False,
        LogLevel level_console=LogLevel.INFO,
        LogLevel level_file=LogLevel.DEBUG,
        LogLevel level_store=LogLevel.WARNING,
        bint run_in_process=False,
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
        level_console : LogLevel (Enum)
            The minimum log level for logging messages to the console.
        level_file : LogLevel (Enum)
            The minimum log level for logging messages to the log file.
        level_store : LogLevel (Enum)
            The minimum log level for storing log messages in memory.
        run_in_process : bool
            If the logger should be run in a separate multiprocessing process.
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

        if run_in_process:
            self._queue = multiprocessing.Queue(maxsize=10000)
            self._process = multiprocessing.Process(target=self._consume_messages, daemon=True)
            self._process.start()
        else:
            self._queue = queue.Queue(maxsize=10000)
            self._thread = threading.Thread(target=self._consume_messages, daemon=True)
            self._thread.start()

    cpdef void log(self, LogMessage message) except *:
        """
        Log the given message.

        If the internal queue is already full then will log a warning and block
        until queue size reduces.

        Parameters
        ----------
        message : LogMessage
            The log message to log.

        """
        Condition.not_none(message, "message")

        try:
            self._queue.put_nowait(message)
        except queue.Full:
            queue_full_msg = LogMessage(
                timestamp=self.clock.utc_now(),
                level=LogLevel.WARNING,
                text=f"LiveLogger: Blocking on `put` as queue full at {self._queue.qsize()} items.",
                thread_id=threading.current_thread().ident,
            )
            self._queue.put(queue_full_msg)
            self._queue.put(message)  # Block until qsize reduces below maxsize

    cpdef void stop(self) except *:
        self._queue.put_nowait(None)  # Sentinel message pattern

    cpdef void _consume_messages(self) except *:
        cdef LogMessage message
        try:
            while True:
                message = self._queue.get()
                if message is None:  # Sentinel message (fast c-level check)
                    break
                self._log(message)
        except KeyboardInterrupt:
            if self._process:
                # Logger is running in a separate process.
                # Here we have caught a single SIGTERM / keyboard interrupt from
                # the main thead, this is to allow final log messages to be
                # processed. Because daemon=True is set, when the main
                # thread exits this will then terminate the process.
                while True:
                    message = self._queue.get()
                    if message is None:
                        break
                    self._log(message)
