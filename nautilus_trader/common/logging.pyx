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

import asyncio
import platform
from platform import python_version
import sys
import traceback

import numpy as np
import pandas as pd
import psutil
import scipy

from cpython.datetime cimport datetime
from nautilus_trader import __version__

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logging cimport LogLevel
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.queue cimport Queue
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

    @staticmethod
    def to_str_py(int value):
        return LogLevelParser.to_str(value)

    @staticmethod
    def from_str_py(str value):
        return LogLevelParser.from_str(value)


cdef class Logger:
    """
    Provides a high-performance logger.
    """

    def __init__(
        self,
        Clock clock not None,
        str name=None,
        LogLevel level_console=LogLevel.INFO,
        bint bypass_logging=False,
    ):
        """
        Initialize a new instance of the `Logger` class.

        Parameters
        ----------
        clock : Clock
            The clock for the logger.
        name : str, optional
            The identifying name of the logger.
        level_console : LogLevel (Enum)
            The minimum log level for logging messages to the console.
        bypass_logging : bool
            If the logger should be bypassed.

        """
        if name is None:
            self.name = ""
        else:
            self.name = name

        self._log_level_console = level_console

        self.clock = clock
        self.is_bypassed = bypass_logging

    cdef str format_text(
        self,
        datetime timestamp,
        LogLevel level,
        LogColor color,
        str text,
    ):
        # Return the formatted log message from the given arguments
        cdef str time = format_iso8601_us(timestamp)
        cdef str colour_cmd = ""

        if color == LogColor.NORMAL:
            pass
        elif color == LogColor.BLUE:
            colour_cmd = _BLUE
        elif color == LogColor.GREEN:
            colour_cmd = _GREEN
        elif color == LogColor.YELLOW:
            colour_cmd = _YELLOW
        elif color == LogColor.RED:
            colour_cmd = _RED

        return (f"{_BOLD}{time}{_ENDC} {colour_cmd}"
                f"[{LogLevelParser.to_str(level)}] {text}{_ENDC}")

    cdef void log_c(self, tuple message) except *:
        self._log(message)

    cdef void _log(self, tuple message) except *:
        cdef LogLevel level = message[0]
        cdef str text = message[1]

        # Sinks
        # -----
        if level >= self._log_level_console:
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
        self.is_bypassed = logger.is_bypassed

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

        self._send_to_logger(LogLevel.VERBOSE, LogColor.NORMAL, message)

    cpdef void debug(self, str message) except *:
        """
        Log the given debug message with the logger.

        Parameters
        ----------
        message : str
            The message to log.

        """
        Condition.not_none(message, "message")

        self._send_to_logger(LogLevel.DEBUG, LogColor.NORMAL, message)

    cpdef void info(self, str message) except *:
        """
        Log the given information message with the logger.

        Parameters
        ----------
        message : str
            The message to log.

        """
        Condition.not_none(message, "message")

        self._send_to_logger(LogLevel.INFO, LogColor.NORMAL, message)

    cpdef void info_blue(self, str message) except *:
        """
        Log the given information message with the logger in blue.

        Parameters
        ----------
        message : str
            The message to log.

        """
        Condition.not_none(message, "message")

        self._send_to_logger(LogLevel.INFO, LogColor.BLUE, message)

    cpdef void info_green(self, str message) except *:
        """
        Log the given information message with the logger in green.

        Parameters
        ----------
        message : str
            The message to log.

        """
        Condition.not_none(message, "message")

        self._send_to_logger(LogLevel.INFO, LogColor.GREEN, message)

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
        if self.is_bypassed:
            return

        cdef str text = self._logger.format_text(
            timestamp=self._logger.clock.utc_now(),
            level=level,
            color=color,
            text=f"{self.component_name}: {message}",
        )

        self._logger.log_c((level, text))


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
    logger.info(f"RAM-Total: {ram_total_mb:,} MB")
    logger.info(f"RAM-Used:  {ram_used__mb:,} MB ({100 - ram_avail_pc:.2f}%)")
    if ram_avail_pc <= 50:
        logger.warning(f"RAM-Avail: {ram_avail_mb:,} MB ({ram_avail_pc:.2f}%)")
    else:
        logger.info(f"RAM-Avail: {ram_avail_mb:,} MB ({ram_avail_pc:.2f}%)")


cdef class LiveLogger(Logger):
    """
    Provides a high-performance logger which runs on the event loop.
    """

    def __init__(
        self,
        loop not None,
        LiveClock clock not None,
        str name=None,
        LogLevel level_console=LogLevel.INFO,
        bint bypass_logging=False,
        int maxsize=10000,
    ):
        """
        Initialize a new instance of the `LiveLogger` class.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop to run the logger on.
        clock : LiveClock
            The clock for the logger.
        name : str, optional
            The identifying name of the logger.
        level_console : LogLevel (Enum)
            The minimum log level for logging messages to the console.
        bypass_logging : bool
            If the logger should be bypassed.
        maxsize : int, optional
            The maximum capacity for the log queue.

        """
        super().__init__(
            clock=clock,
            name=name,
            level_console=level_console,
            bypass_logging=bypass_logging,
        )

        self._loop = loop
        self._queue = Queue(maxsize=maxsize)

        self._run_task = None
        self.is_running = False

    cdef void log_c(self, tuple message) except *:
        """
        Log the given message.

        If the internal queue is already full then will log a warning and block
        until queue size reduces.

        If the event loop is not running then messages will be passed directly
        to the `Logger` base class for logging.

        Parameters
        ----------
        message : tuple(int, str)
            The log level and text.

        """
        Condition.not_none(message, "message")

        if self.is_running:
            try:
                self._queue.put_nowait(message)
            except asyncio.QueueFull:
                text = self.format_text(
                    timestamp=self.clock.utc_now(),
                    level=LogLevel.WARNING,
                    color=LogColor.YELLOW,
                    text=f"LiveLogger: Blocking on `_queue.put` as queue full at {self._queue.qsize()} items.",
                )
                self._log((LogLevel.WARNING, text))
                self._loop.create_task(self._queue.put(message))  # Blocking until qsize reduces
        else:
            # If event loop is not running then pass message directly to the
            # base class to log.
            self._log(message)

    cpdef void start(self) except *:
        """
        Start the logger on a running event loop.
        """
        if not self.is_running:
            self._run_task = self._loop.create_task(self._consume_messages())
        self.is_running = True

    cpdef void stop(self) except *:
        """
        Stop the logger by cancelling the internal event loop task.

        Future messages sent to the logger will be passed directly to the
        `Logger` base class for logging.

        """
        if self._run_task:
            self._run_task.cancel()
        self.is_running = False

    async def _consume_messages(self):
        try:
            while True:
                self._log(await self._queue.get())
        except asyncio.CancelledError:
            pass
        finally:
            # Pass remaining messages directly to the base class
            while not self._queue.empty():
                self._log(self._queue.get_nowait())
