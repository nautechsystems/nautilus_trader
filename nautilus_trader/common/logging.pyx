# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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
import socket
import sys
import traceback
from asyncio import Task
from collections import defaultdict
from platform import python_version
from typing import Optional

import aiohttp
import msgspec
import numpy as np
import orjson
import pandas as pd
import psutil
import pyarrow
import pydantic
import pytz

from nautilus_trader import __version__

from cpython.datetime cimport timedelta

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LogLevel
from nautilus_trader.common.queue cimport Queue
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport format_iso8601_ns
from nautilus_trader.model.identifiers cimport TraderId


# ANSI color constants
cdef str _HEADER = "\033[95m"
cdef str _GREEN = "\033[92m"
cdef str _BLUE = "\033[94m"
cdef str _MAGENTA = "\033[35m"
cdef str _CYAN = "\033[36m"
cdef str _YELLOW = "\033[1;33m"
cdef str _RED = "\033[1;31m"
cdef str _ENDC = "\033[0m"
cdef str _BOLD = "\033[1m"
cdef str _UNDERLINE = "\033[4m"

RECV = "<--"
SENT = "-->"
CMD = "[CMD]"
EVT = "[EVT]"
DOC = "[DOC]"
RPT = "[RPT]"
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
        if value == "DBG" or value == "DEBUG":
            return LogLevel.DEBUG
        elif value == "INF" or value == "INFO":
            return LogLevel.INFO
        elif value == "WRN" or value == "WARNING":
            return LogLevel.WARNING
        elif value == "ERR" or value == "ERROR":
            return LogLevel.ERROR
        elif value == "CRT" or value == "CRITICAL":
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

    Parameters
    ----------
    clock : Clock
        The clock for the logger.
    trader_id : TraderId, optional
        The trader ID for the logger.
    machine_id : str, optional
        The machine ID.
    instance_id : UUID4, optional
        The instance ID.
    level_stdout : LogLevel
        The minimum log level for logging messages to stdout.
    bypass : bool
        If the logger should be bypassed.
    """

    def __init__(
        self,
        Clock clock not None,
        TraderId trader_id=None,
        str machine_id=None,
        UUID4 instance_id=None,
        LogLevel level_stdout=LogLevel.INFO,
        bint bypass=False,
    ):
        if trader_id is None:
            trader_id = TraderId("TRADER-000")
        if instance_id is None:
            instance_id = UUIDFactory().generate()
        if machine_id is None:
            machine_id = socket.gethostname()

        self._clock = clock
        self._log_level_stdout = level_stdout
        self._sinks = []

        self.trader_id = trader_id
        self.machine_id = machine_id
        self.instance_id = instance_id
        self.is_bypassed = bypass

    cpdef void register_sink(self, handler: Callable[[Dict], None]) except *:
        """
        Register the given sink handler with the logger.

        Parameters
        ----------
        handler : Callable[[Dict], None]
            The sink handler to register.

        Raises
        ------
        KeyError
            If `handler` already registered.

        """
        Condition.not_none(handler, "handler")
        Condition.not_in(handler, self._sinks, "handler", "_sinks")

        self._sinks.append(handler)

    cdef void change_clock_c(self, Clock clock) except *:
        """
        Change the loggers internal clock to the given clock.

        Parameters
        ----------
        clock : Clock

        """
        Condition.not_none(clock, "clock")

        self._clock = clock

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
            "trader_id": self.trader_id.value,
            "machine_id": self.machine_id,
            "instance_id": self.instance_id.to_str(),
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

        if self._sinks:
            del record["color"]  # Remove redundant color tag
            for handler in self._sinks:
                handler(record)

    cdef str _format_record(
        self,
        LogLevel level,
        LogColor color,
        dict record,
    ):
        # Set log color
        cdef str color_cmd = ""
        if color == LogColor.NORMAL:
            pass
        if color == LogColor.BLUE:
            color_cmd = _BLUE
        elif color == LogColor.GREEN:
            color_cmd = _GREEN
        elif color == LogColor.MAGENTA:
            color_cmd = _MAGENTA
        elif color == LogColor.CYAN:
            color_cmd = _CYAN
        elif color == LogColor.YELLOW:
            color_cmd = _YELLOW
        elif color == LogColor.RED:
            color_cmd = _RED

        # Return the formatted log message from the given arguments
        cdef str dt = format_iso8601_ns(pd.Timestamp(record["timestamp"], tz="UTC"))
        cdef str trader_id_str = f"{self.trader_id.value}." if self.trader_id is not None else ""
        return (
            f"{_BOLD}{dt}{_ENDC} {color_cmd}"
            f"[{LogLevelParser.to_str(level)}] "
            f"{trader_id_str}{record['component']}: {record['msg']}{_ENDC}"
        )


cdef class LoggerAdapter:
    """
    Provides an adapter for a components logger.

    Parameters
    ----------
    component_name : str
        The name of the component.
    logger : Logger
        The logger for the component.
    """

    def __init__(
        self,
        str component_name not None,
        Logger logger not None,
    ):
        Condition.valid_string(component_name, "component_name")

        self._logger = logger
        self.trader_id = logger.trader_id
        self.machine_id = logger.machine_id
        self.instance_id = logger.instance_id
        self.component = component_name
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

    cpdef void exception(
        self,
        str msg,
        ex,
        dict annotations=None,
    ) except *:
        """
        Log the given exception including stack trace information.

        Parameters
        ----------
        msg : str
            The message to log.
        ex : Exception
            The exception to log.
        annotations : dict[str, object], optional
            The annotations for the log record.

        """
        Condition.not_none(ex, "ex")

        cdef str ex_string = f"{type(ex).__name__}({ex})"
        ex_type, ex_value, ex_traceback = sys.exc_info()
        stack_trace = traceback.format_exception(ex_type, ex_value, ex_traceback)

        cdef str stack_trace_lines = ""
        cdef str line
        for line in stack_trace[:len(stack_trace) - 1]:
            stack_trace_lines += line

        self.error(f"{msg}\n{ex_string}\n{stack_trace_lines}", annotations=annotations)


cpdef void nautilus_header(LoggerAdapter logger) except *:
    Condition.not_none(logger, "logger")
    print("")  # New line to begin
    logger.info("\033[36m=================================================================")
    logger.info(f"\033[36m NAUTILUS TRADER - Automated Algorithmic Trading Platform")
    logger.info(f"\033[36m by Nautech Systems Pty Ltd.")
    logger.info(f"\033[36m Copyright (C) 2015-2022. All rights reserved.")
    logger.info("\033[36m=================================================================")
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
    logger.info("\033[36m=================================================================")
    logger.info("\033[36m SYSTEM SPECIFICATION")
    logger.info("\033[36m=================================================================")
    logger.info(f"CPU architecture: {platform.processor()}")
    try:
        cpu_freq_str = f"@ {int(psutil.cpu_freq()[2])} MHz"
    except Exception:  # noqa (historically problematic call on ARM)
        cpu_freq_str = None
    logger.info(f"CPU(s): {psutil.cpu_count()} {cpu_freq_str}")
    logger.info(f"OS: {platform.platform()}")
    log_memory(logger)
    logger.info("\033[36m=================================================================")
    logger.info("\033[36m IDENTIFIERS")
    logger.info("\033[36m=================================================================")
    logger.info(f"trader_id: {logger.trader_id}")
    logger.info(f"machine_id: {logger.machine_id}")
    logger.info(f"instance_id: {logger.instance_id}")
    logger.info("\033[36m=================================================================")
    logger.info("\033[36m VERSIONING")
    logger.info("\033[36m=================================================================")
    logger.info(f"nautilus-trader {__version__}")
    logger.info(f"python {python_version()}")
    logger.info(f"numpy {np.__version__}")
    logger.info(f"pandas {pd.__version__}")
    logger.info(f"aiohttp {aiohttp.__version__}")
    logger.info(f"msgspec {msgspec.__version__}")
    logger.info(f"orjson {orjson.__version__}")
    logger.info(f"psutil {psutil.__version__}")
    logger.info(f"pyarrow {pyarrow.__version__}")
    logger.info(f"pydantic {pydantic.__version__}")
    logger.info(f"pytz {pytz.__version__}")  # type: ignore
    try:
        import redis
        logger.info(f"redis {redis.__version__}")
    except ImportError:  # pragma: no cover
        redis = None
    try:
        import hiredis
        logger.info(f"hiredis {hiredis.__version__}")
    except ImportError:  # pragma: no cover
        hiredis = None
    try:
        import uvloop
        logger.info(f"uvloop {uvloop.__version__}")
    except ImportError:  # pragma: no cover
        uvloop = None

    logger.info("\033[36m=================================================================")

cpdef void log_memory(LoggerAdapter logger) except *:
    logger.info("\033[36m=================================================================")
    logger.info("\033[36m MEMORY USAGE")
    logger.info("\033[36m=================================================================")
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

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop to run the logger on.
    clock : LiveClock
        The clock for the logger.
    trader_id : TraderId, optional
        The trader ID for the logger.
    machine_id : str, optional
        The machine ID for the logger.
    instance_id : UUID4, optional
        The systems unique instantiation ID.
    level_stdout : LogLevel
        The minimum log level for logging messages to stdout.
    bypass : bool
        If the logger should be bypassed.
    maxsize : int, optional
        The maximum capacity for the log queue.
    """
    _sentinel = None

    def __init__(
        self,
        loop not None,
        LiveClock clock not None,
        TraderId trader_id=None,
        str machine_id=None,
        UUID4 instance_id=None,
        LogLevel level_stdout=LogLevel.INFO,
        bint bypass=False,
        int maxsize=10000,
    ):
        super().__init__(
            clock=clock,
            trader_id=trader_id,
            machine_id=machine_id,
            instance_id=instance_id,
            level_stdout=level_stdout,
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
