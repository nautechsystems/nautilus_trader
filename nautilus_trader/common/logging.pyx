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

from typing import Optional

from cpython.datetime cimport timedelta

import asyncio
from asyncio import Task
from collections import defaultdict
import platform
from platform import python_version
import sys
import traceback

import numpy as np
import pandas as pd
import psutil
import scipy

from nautilus_trader import __version__

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logging cimport LogLevel
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.queue cimport Queue
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport format_iso8601_us
from nautilus_trader.core.datetime cimport nanos_to_unix_dt
from nautilus_trader.model.identifiers cimport TraderId


# ANSI color constants
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
        if value == 10:
            return "DBG"
        elif value == 20:
            return "INF"
        elif value == 30:
            return "WRN"
        elif value == 40:
            return "ERR"
        elif value == 50:
            return "CRT"

    @staticmethod
    cdef LogLevel from_str(str value):
        if value == "DBG":
            return LogLevel.DEBUG
        elif value == "INF":
            return LogLevel.INFO
        elif value == "WRN":
            return LogLevel.WARNING
        elif value == "ERR":
            return LogLevel.ERROR
        elif value == "CRT":
            return LogLevel.CRITICAL

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
        TraderId trader_id=None,
        UUID system_id=None,
        LogLevel level_stdout=LogLevel.INFO,
        LogLevel level_raw=LogLevel.DEBUG,
        bint bypass=False,
    ):
        """
        Initialize a new instance of the ``Logger`` class.

        Parameters
        ----------
        clock : Clock
            The clock for the logger.
        trader_id : TraderId, optional
            The trader ID for the logger.
        system_id : UUID, optional
            The systems unique instantiation ID.
        level_stdout : LogLevel
            The minimum log level for logging messages to stdout.
        level_raw : LogLevel
            The minimum log level for the raw log record sink.
        bypass : bool
            If the logger should be bypassed.

        """
        if system_id is None:
            system_id = UUIDFactory().generate()
        self._clock = clock
        self._log_level_stdout = level_stdout
        self._log_level_raw = level_raw

        self.trader_id = trader_id
        self.system_id = system_id
        self.is_bypassed = bypass

    cdef void log_c(self, dict record) except *:
        """
        Handle the given record by sending it to configured sinks.

        Override this method to handle log records through a `Queue`.

        Parameters
        ----------
        record : dict[str, object]

        """
        self._log(record)

    cdef dict create_record(
        self,
        LogLevel level,
        LogColor color,
        str component,
        str msg,
        dict annotations=None,
    ):
        cdef dict record = {
            "timestamp": self._clock.timestamp_ns(),
            "level": LogLevelParser.to_str(level),
            "color": color,
            "trader_id": self.trader_id.value if self.trader_id is not None else "",
            "system_id": self.system_id.value,
            "component": component,
            "msg": msg,
        }

        if annotations is not None:
            record = {**record, **annotations}

        return record

    cdef void _log(self, dict record) except *:
        cdef LogLevel level = LogLevelParser.from_str(record["level"])
        cdef LogColor color = record.get("color", 0)

        if level >= LogLevel.ERROR:
            sys.stderr.write(f"{self._format_record(level, color, record)}\n")
        elif level >= self._log_level_stdout:
            sys.stdout.write(f"{self._format_record(level, color, record)}\n")

        if level >= self._log_level_raw:
            pass  # TODO: Raw sink out - str(record)

    cdef str _format_record(
        self,
        LogLevel level,
        LogColor color,
        dict record,
    ):
        # Return the formatted log message from the given arguments
        cdef str time = format_iso8601_us(nanos_to_unix_dt(record["timestamp"]))

        # Set log color
        cdef str color_cmd = ""
        if color == LogColor.YELLOW:
            color_cmd = _YELLOW
        elif color == LogColor.GREEN:
            color_cmd = _GREEN
        elif color == LogColor.BLUE:
            color_cmd = _BLUE
        elif color == LogColor.RED:
            color_cmd = _RED

        cdef str trader_id_str = f"{self.trader_id.value}." if self.trader_id is not None else ""
        return (f"{_BOLD}{time}{_ENDC} {color_cmd}"
                f"[{LogLevelParser.to_str(level)}] "
                f"{trader_id_str}{record['component']}: {record['msg']}{_ENDC}")


cdef class LoggerAdapter:
    """
    Provides an adapter for a components logger.
    """

    def __init__(
        self,
        str component not None,
        Logger logger not None,
    ):
        """
        Initialize a new instance of the ``LoggerAdapter`` class.

        Parameters
        ----------
        component : str
            The name of the component.
        logger : Logger
            The logger for the component.

        """
        Condition.valid_string(component, "component")

        self._logger = logger
        self.component = component
        self.is_bypassed = logger.is_bypassed

    cpdef Logger get_logger(self):
        """
        Return the encapsulated logger.

        Returns
        -------
        Logger

        """
        return self._logger

    cpdef void debug(
        self,
        str msg,
        LogColor color=LogColor.NORMAL,
        dict annotations=None,
    ) except *:
        """
        Log the given debug message with the logger.

        Parameters
        ----------
        msg : str
            The message to log.
        color : LogColor, optional
            The color for the log record.
        annotations : dict[str, object], optional
            The annotations for the log record.

        """
        Condition.not_none(msg, "message")

        if self.is_bypassed:
            return

        cdef dict record = self._logger.create_record(
            level=LogLevel.DEBUG,
            color=color,
            component=self.component,
            msg=msg,
            annotations=annotations,
        )

        self._logger.log_c(record)

    cpdef void info(
        self, str msg,
        LogColor color=LogColor.NORMAL,
        dict annotations=None,
    ) except *:
        """
        Log the given information message with the logger.

        Parameters
        ----------
        msg : str
            The message to log.
        color : LogColor, optional
            The color for the log record.
        annotations : dict[str, object], optional
            The annotations for the log record.

        """
        Condition.not_none(msg, "msg")

        if self.is_bypassed:
            return

        cdef dict record = self._logger.create_record(
            level=LogLevel.INFO,
            color=color,
            component=self.component,
            msg=msg,
            annotations=annotations,
        )

        self._logger.log_c(record)

    cpdef void warning(
        self,
        str msg,
        LogColor color=LogColor.YELLOW,
        dict annotations=None,
    ) except *:
        """
        Log the given warning message with the logger.

        Parameters
        ----------
        msg : str
            The message to log.
        color : LogColor, optional
            The color for the log record.
        annotations : dict[str, object], optional
            The annotations for the log record.

        """
        Condition.not_none(msg, "msg")

        if self.is_bypassed:
            return

        cdef dict record = self._logger.create_record(
            level=LogLevel.WARNING,
            color=color,
            component=self.component,
            msg=msg,
            annotations=annotations,
        )

        self._logger.log_c(record)

    cpdef void error(
        self,
        str msg,
        LogColor color=LogColor.RED,
        dict annotations=None,
    ) except *:
        """
        Log the given error message with the logger.

        Parameters
        ----------
        msg : str
            The message to log.
        color : LogColor, optional
            The color for the log record.
        annotations : dict[str, object], optional
            The annotations for the log record.

        """
        Condition.not_none(msg, "msg")

        if self.is_bypassed:
            return

        cdef dict record = self._logger.create_record(
            level=LogLevel.ERROR,
            color=color,
            component=self.component,
            msg=msg,
            annotations=annotations,
        )

        self._logger.log_c(record)

    cpdef void critical(
        self,
        str msg,
        LogColor color=LogColor.RED,
        dict annotations=None,
    ) except *:
        """
        Log the given critical message with the logger.

        Parameters
        ----------
        msg : str
            The message to log.
        color : LogColor, optional
            The color for the log record.
        annotations : dict[str, object], optional
            The annotations for the log record.

        """
        Condition.not_none(msg, "msg")

        if self.is_bypassed:
            return

        cdef dict record = self._logger.create_record(
            level=LogLevel.CRITICAL,
            color=color,
            component=self.component,
            msg=msg,
            annotations=annotations,
        )

        self._logger.log_c(record)

    cpdef void exception(self, ex, dict annotations=None) except *:
        """
        Log the given exception including stack trace information.

        Parameters
        ----------
        ex : Exception
            The message to log.
        annotations : dict[str, object], optional
            The annotations for the log record.

        """
        Condition.not_none(ex, "ex")

        cdef str ex_string = f"{type(ex).__name__}({ex})\n"
        ex_type, ex_value, ex_traceback = sys.exc_info()
        stack_trace = traceback.format_exception(ex_type, ex_value, ex_traceback)

        cdef str stack_trace_lines = ""
        cdef str line
        for line in stack_trace[:len(stack_trace) - 1]:
            stack_trace_lines += line

        self.error(f"{ex_string} {stack_trace_lines}", annotations=annotations)


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
    logger.info(" IDENTIFIERS")
    logger.info("=================================================================")
    logger.info(f"trader_id: {logger.get_logger().trader_id.value}")
    logger.info(f"system_id: {logger.get_logger().system_id.value}")
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
    _sentinel = None

    def __init__(
        self,
        loop not None,
        LiveClock clock not None,
        TraderId trader_id=None,
        UUID system_id=None,
        LogLevel level_stdout=LogLevel.INFO,
        LogLevel level_raw=LogLevel.DEBUG,
        bint bypass=False,
        int maxsize=10000,
    ):
        """
        Initialize a new instance of the ``LiveLogger`` class.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop to run the logger on.
        clock : LiveClock
            The clock for the logger.
        trader_id : TraderId, optional
            The trader ID for the logger.
        system_id : UUID, optional
            The systems unique instantiation ID.
        level_stdout : LogLevel
            The minimum log level for logging messages to stdout.
        level_raw : LogLevel
            The minimum log level for the raw log record sink.
        bypass : bool
            If the logger should be bypassed.
        maxsize : int, optional
            The maximum capacity for the log queue.

        """
        super().__init__(
            clock=clock,
            trader_id=trader_id,
            system_id=system_id,
            level_stdout=level_stdout,
            level_raw=level_raw,
            bypass=bypass,
        )

        self._loop = loop
        self._queue = Queue(maxsize=maxsize)
        self._run_task: Optional[Task] = None
        self._blocked_log_interval = timedelta(seconds=1)

        self.is_running = False
        self.last_blocked: Optional[datetime] = None

    def get_run_task(self) -> asyncio.Task:
        """
        Return the internal run queue task for the engine.

        Returns
        -------
        asyncio.Task

        """
        return self._run_task

    cdef void log_c(self, dict record) except *:
        """
        Log the given message.

        If the internal queue is already full then will log a warning and block
        until queue size reduces.

        If the event loop is not running then messages will be passed directly
        to the `Logger` base class for logging.

        Parameters
        ----------
        record : dict[str, object]
            The log record.

        """
        Condition.not_none(record, "record")

        if self.is_running:
            try:
                self._queue.put_nowait(record)
            except asyncio.QueueFull:
                now = self._clock.utc_now()
                next_msg = self._queue.peek_front().get("msg")

                # Log blocking message once a second
                if (
                    self.last_blocked is None
                    or now >= self.last_blocked + self._blocked_log_interval
                ):
                    self.last_blocked = now

                    messages = [r["msg"] for r in self._queue.to_list()]
                    message_types = defaultdict(lambda: 0)
                    for msg in messages:
                        message_types[msg] += 1
                    sorted_types = sorted(
                        message_types.items(),
                        key=lambda kv: kv[1],
                        reverse=True,
                    )

                    blocked_msg = '\n'.join([f"'{kv[0]}' [x{kv[1]}]" for kv in sorted_types])
                    blocking_record = self.create_record(
                        level=LogLevel.WARNING,
                        color=LogColor.YELLOW,
                        component=type(self).__name__,
                        msg=f"Blocking full log queue at "
                            f"{self._queue.qsize()} items. "
                            f"\nNext msg = '{next_msg}'.\n{blocked_msg}",
                    )

                    self._log(blocking_record)

                # If not spamming then add record to event loop
                if next_msg != record.get("msg"):
                    self._loop.create_task(self._queue.put(record))  # Blocking until qsize reduces
        else:
            # If event loop is not running then pass message directly to the
            # base class to log.
            self._log(record)

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
            self.is_running = False
            self._enqueue_sentinel()

    async def _consume_messages(self):
        cdef dict record
        try:
            while self.is_running:
                record = await self._queue.get()
                if record is None:  # Sentinel message (fast C-level check)
                    continue        # Returns to the top to check `self.is_running`
                self._log(record)
        except asyncio.CancelledError:
            pass
        finally:
            # Pass remaining messages directly to the base class
            while not self._queue.empty():
                record = self._queue.get_nowait()
                if record:
                    self._log(record)

    cdef void _enqueue_sentinel(self) except *:
        self._queue.put_nowait(self._sentinel)
