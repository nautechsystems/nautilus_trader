# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
import pandas as pd
import psutil
import pyarrow
import pytz

from nautilus_trader import __version__

from cpython.datetime cimport timedelta
from libc.stdint cimport uint64_t

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.enums_c cimport log_level_to_str
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.queue cimport Queue
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.rust.common cimport LogColor
from nautilus_trader.core.rust.common cimport LogLevel
from nautilus_trader.core.rust.common cimport logger_free
from nautilus_trader.core.rust.common cimport logger_get_instance_id
from nautilus_trader.core.rust.common cimport logger_get_machine_id_cstr
from nautilus_trader.core.rust.common cimport logger_get_trader_id_cstr
from nautilus_trader.core.rust.common cimport logger_is_bypassed
from nautilus_trader.core.rust.common cimport logger_log
from nautilus_trader.core.rust.common cimport logger_new
from nautilus_trader.core.string cimport cstr_to_pystr
from nautilus_trader.core.string cimport pystr_to_cstr
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.model.identifiers cimport TraderId


RECV = "<--"
SENT = "-->"
CMD = "[CMD]"
EVT = "[EVT]"
DOC = "[DOC]"
RPT = "[RPT]"
REQ = "[REQ]"
RES = "[RES]"


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
        TraderId trader_id = None,
        str machine_id = None,
        UUID4 instance_id = None,
        LogLevel level_stdout = LogLevel.INFO,
        bint bypass = False,
    ):
        if trader_id is None:
            trader_id = TraderId("TRADER-000")
        if instance_id is None:
            instance_id = UUID4()
        if machine_id is None:
            machine_id = socket.gethostname()

        self._clock = clock

        cdef str trader_id_str = trader_id.to_str()
        cdef str instance_id_str = instance_id.to_str()
        self._mem = logger_new(
            pystr_to_cstr(trader_id_str),
            pystr_to_cstr(machine_id),
            pystr_to_cstr(instance_id_str),
            level_stdout,
            bypass,
        )
        self._sinks = []

    def __del__(self) -> None:
        if self._mem._0 != NULL:
            logger_free(self._mem)  # `self._mem` moved to Rust (then dropped)

    @property
    def trader_id(self) -> TraderId:
        """
        Return the loggers trader ID.

        Returns
        -------
        TraderId

        """
        return TraderId(cstr_to_pystr(logger_get_trader_id_cstr(&self._mem)))

    @property
    def machine_id(self) -> str:
        """
        Return the loggers machine ID.

        Returns
        -------
        str

        """
        return cstr_to_pystr(logger_get_machine_id_cstr(&self._mem))

    @property
    def instance_id(self) -> UUID4:
        """
        Return the loggers system instance ID.

        Returns
        -------
        UUID4

        """
        return UUID4.from_mem_c(logger_get_instance_id(&self._mem))

    @property
    def is_bypassed(self) -> bool:
        """
        Return whether the logger is in bypass mode.

        Returns
        -------
        bool

        """
        return logger_is_bypassed(&self._mem)

    cpdef void register_sink(self, handler: Callable[[dict], None]) except *:
        """
        Register the given sink handler with the logger.

        Parameters
        ----------
        handler : Callable[[dict], None]
            The sink handler to register.

        Raises
        ------
        KeyError
            If `handler` already registered.

        """
        Condition.not_none(handler, "handler")
        Condition.not_in(handler, self._sinks, "handler", "_sinks")

        self._sinks.append(handler)

    cpdef void change_clock(self, Clock clock) except *:
        """
        Change the loggers internal clock to the given clock.

        Parameters
        ----------
        clock : Clock

        """
        Condition.not_none(clock, "clock")

        self._clock = clock

    cdef dict create_record(
        self,
        LogLevel level,
        str component,
        str msg,
        dict annotations = None,
    ):
        cdef dict record = {
            "timestamp": self._clock.timestamp_ns(),
            "level": log_level_to_str(level),
            "trader_id": str(self.trader_id),
            "machine_id": self.machine_id,
            "instance_id": str(self.instance_id),
            "component": component,
            "msg": msg,
        }

        if annotations is not None:
            record = {**record, **annotations}

        return record

    cdef void log(
        self,
        uint64_t timestamp_ns,
        LogLevel level,
        LogColor color,
        str component,
        str msg,
        dict annotations = None,
    ) except *:
        self._log(
            timestamp_ns,
            level,
            color,
            component,
            msg,
            annotations,
        )

    cdef void _log(
        self,
        uint64_t timestamp_ns,
        LogLevel level,
        LogColor color,
        str component,
        str msg,
        dict annotations,
    ) except *:
        logger_log(
            &self._mem,
            timestamp_ns,
            level,
            color,
            pystr_to_cstr(component),
            pystr_to_cstr(msg),
        )

        if not self._sinks:
            return

        cdef dict record = self.create_record(
            level=level,
            component=component,
            msg=msg,
            annotations=annotations,
        )

        for handler in self._sinks:
            handler(record)


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
        self._component = component_name
        self._is_bypassed = logger.is_bypassed

    @property
    def trader_id(self) -> TraderId:
        """
        Return the loggers trader ID.

        Returns
        -------
        TraderId

        """
        return self._logger.trader_id

    @property
    def machine_id(self) -> str:
        """
        Return the loggers machine ID.

        Returns
        -------
        str

        """
        return self._logger.machine_id

    @property
    def instance_id(self) -> UUID4:
        """
        Return the loggers system instance ID.

        Returns
        -------
        UUID4

        """
        return self._logger.instance_id

    @property
    def component(self) -> str:
        """
        Return the loggers component name.

        Returns
        -------
        str

        """
        return self._component

    @property
    def is_bypassed(self) -> str:
        """
        Return whether the logger is in bypass mode.

        Returns
        -------
        str

        """
        return self._is_bypassed

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
        LogColor color = LogColor.NORMAL,
        dict annotations = None,
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

        self._logger.log(
            self._logger._clock.timestamp_ns(),
            LogLevel.DEBUG,
            color,
            self.component,
            msg,
            annotations,
        )

    cpdef void info(
        self, str msg,
        LogColor color = LogColor.NORMAL,
        dict annotations = None,
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

        self._logger.log(
            self._logger._clock.timestamp_ns(),
            LogLevel.INFO,
            color,
            self.component,
            msg,
            annotations,
        )

    cpdef void warning(
        self,
        str msg,
        LogColor color = LogColor.YELLOW,
        dict annotations = None,
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

        self._logger.log(
            self._logger._clock.timestamp_ns(),
            LogLevel.WARNING,
            color,
            self.component,
            msg,
            annotations,
        )

    cpdef void error(
        self,
        str msg,
        LogColor color = LogColor.RED,
        dict annotations = None,
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

        self._logger.log(
            self._logger._clock.timestamp_ns(),
            LogLevel.ERROR,
            color,
            self.component,
            msg,
            annotations,
        )

    cpdef void critical(
        self,
        str msg,
        LogColor color = LogColor.RED,
        dict annotations = None,
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

        self._logger.log(
            self._logger._clock.timestamp_ns(),
            LogLevel.CRITICAL,
            color,
            self.component,
            msg,
            annotations,
        )

    cpdef void exception(
        self,
        str msg,
        ex,
        dict annotations = None,
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
    logger.info(f"\033[36m Copyright (C) 2015-2023. All rights reserved.")
    logger.info("\033[36m=================================================================")
    logger.info("")
    logger.info("⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣠⣴⣶⡟⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀")
    logger.info("⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣰⣾⣿⣿⣿⠀⢸⣿⣿⣿⣿⣶⣶⣤⣀⠀⠀⠀⠀⠀")
    logger.info("⠀⠀⠀⠀⠀⠀⢀⣴⡇⢀⣾⣿⣿⣿⣿⣿⠀⣾⣿⣿⣿⣿⣿⣿⣿⠿⠓⠀⠀⠀⠀")
    logger.info("⠀⠀⠀⠀⠀⣰⣿⣿⡀⢸⣿⣿⣿⣿⣿⣿⠀⣿⣿⣿⣿⣿⣿⠟⠁⣠⣄⠀⠀⠀⠀")
    logger.info("⠀⠀⠀⠀⢠⣿⣿⣿⣇⠀⢿⣿⣿⣿⣿⣿⠀⢻⣿⣿⣿⡿⢃⣠⣾⣿⣿⣧⡀⠀⠀")
    logger.info("⠀⠀⠀⠀⢸⣿⣿⣿⣿⣆⠘⢿⣿⡿⠛⢉⠀⠀⠉⠙⠛⣠⣿⣿⣿⣿⣿⣿⣷⠀⠀")
    logger.info("⠀⠀⠀⠠⣾⣿⣿⣿⣿⣿⣧⠈⠋⢀⣴⣧⠀⣿⡏⢠⡀⢸⣿⣿⣿⣿⣿⣿⣿⡇⠀")
    logger.info("⠀⠀⠀⣀⠙⢿⣿⣿⣿⣿⣿⠇⢠⣿⣿⣿⡄⠹⠃⠼⠃⠈⠉⠛⠛⠛⠛⠛⠻⠇⠀")
    logger.info("⠀⠀⢸⡟⢠⣤⠉⠛⠿⢿⣿⠀⢸⣿⡿⠋⣠⣤⣄⠀⣾⣿⣿⣶⣶⣶⣦⡄⠀⠀⠀")
    logger.info("⠀⠀⠸⠀⣾⠏⣸⣷⠂⣠⣤⠀⠘⢁⣴⣾⣿⣿⣿⡆⠘⣿⣿⣿⣿⣿⣿⠀⠀⠀⠀")
    logger.info("⠀⠀⠀⠀⠛⠀⣿⡟⠀⢻⣿⡄⠸⣿⣿⣿⣿⣿⣿⣿⡀⠘⣿⣿⣿⣿⠟⠀⠀⠀⠀")
    logger.info("⠀⠀⠀⠀⠀⠀⣿⠇⠀⠀⢻⡿⠀⠈⠻⣿⣿⣿⣿⣿⡇⠀⢹⣿⠿⠋⠀⠀⠀⠀⠀")
    logger.info("⠀⠀⠀⠀⠀⠀⠋⠀⠀⠀⡘⠁⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠈⠁⠀⠀⠀⠀⠀⠀⠀")
    logger.info("")
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
    logger.info(f"psutil {psutil.__version__}")
    logger.info(f"pyarrow {pyarrow.__version__}")
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
        TraderId trader_id = None,
        str machine_id = None,
        UUID4 instance_id = None,
        LogLevel level_stdout = LogLevel.INFO,
        bint bypass = False,
        int maxsize = 10000,
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

        self._is_running = False
        self._last_blocked: Optional[datetime] = None

    @property
    def is_running(self) -> bool:
        """
        Return whether the logger is running.

        Returns
        -------
        bool

        """
        return self._is_running

    @property
    def last_blocked(self) -> Optional[datetime]:
        """
        Return the timestamp (UTC) the logger last blocked.

        Returns
        -------
        datetime or ``None``

        """
        return self._last_blocked

    def get_run_task(self) -> asyncio.Task:
        """
        Return the internal run queue task for the engine.

        Returns
        -------
        asyncio.Task

        """
        return self._run_task

    cdef void log(
        self,
        uint64_t timestamp_ns,
        LogLevel level,
        LogColor color,
        str component,
        str msg,
        dict annotations = None,
    ) except *:
        """
        Log the given message.

        If the internal queue is already full then will log a warning and block
        until queue size reduces.

        If the event loop is not running then messages will be passed directly
        to the `Logger` base class for logging.

        """
        Condition.not_none(component, "component")
        Condition.not_none(msg, "msg")

        if self._is_running:
            try:
                self._queue.put_nowait((timestamp_ns, level, color, component, msg, annotations))
            except asyncio.QueueFull:
                now = self._clock.utc_now()
                next_msg = self._queue.peek_front()[4]

                # Log blocking message once a second
                if (
                    self._last_blocked is None
                    or now >= self._last_blocked + self._blocked_log_interval
                ):
                    self._last_blocked = now

                    messages = [r[4] for r in self._queue.to_list()]
                    message_types = defaultdict(lambda: 0)
                    for msg in messages:
                        message_types[msg] += 1
                    sorted_types = sorted(
                        message_types.items(),
                        key=lambda kv: kv[1],
                        reverse=True,
                    )

                    blocked_msg = '\n'.join([f"'{kv[0]}' [x{kv[1]}]" for kv in sorted_types])
                    log_msg = (f"Blocking full log queue at "
                               f"{self._queue.qsize()} items. "
                               f"\nNext msg = '{next_msg}'.\n{blocked_msg}")

                    self._log(
                        timestamp_ns,
                        LogLevel.WARNING,
                        LogColor.YELLOW,
                        type(self).__name__,
                        log_msg,
                        annotations,
                    )

                # If not spamming then add record to event loop
                if next_msg != msg:
                    self._loop.create_task(self._queue.put((timestamp_ns, level, color, component, msg, annotations)))  # Blocking until qsize reduces
        else:
            # If event loop is not running then pass message directly to the
            # base class to log.
            self._log(
                timestamp_ns,
                level,
                color,
                component,
                msg,
                annotations,
            )

    cpdef void start(self) except *:
        """
        Start the logger on a running event loop.
        """
        if not self._is_running:
            self._run_task = self._loop.create_task(self._consume_messages())
        self._is_running = True

    cpdef void stop(self) except *:
        """
        Stop the logger by canceling the internal event loop task.

        Future messages sent to the logger will be passed directly to the
        `Logger` base class for logging.

        """
        if self._run_task:
            self._is_running = False
            self._enqueue_sentinel()

    async def _consume_messages(self):
        cdef tuple record
        try:
            while self._is_running:
                record = await self._queue.get()
                if record is None:  # Sentinel message (fast C-level check)
                    continue        # Returns to the top to check `self._is_running`
                self._log(
                    record[0],
                    record[1],
                    record[2],
                    record[3],
                    record[4],
                    record[5],
                )
        except asyncio.CancelledError:
            pass
        finally:
            # Pass remaining messages directly to the base class
            while not self._queue.empty():
                record = self._queue.get_nowait()
                if record:
                    self._log(
                        record[0],
                        record[1],
                        record[2],
                        record[3],
                        record[4],
                        record[5],
                    )

    cdef void _enqueue_sentinel(self) except *:
        self._queue.put_nowait(self._sentinel)
