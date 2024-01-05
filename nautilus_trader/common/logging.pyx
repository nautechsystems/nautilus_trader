# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

import platform
import socket
import sys
import traceback
from platform import python_version

import msgspec
import numpy as np
import pandas as pd
import psutil
import pyarrow
import pytz

from nautilus_trader import __version__

from libc.stdint cimport uint64_t

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.rust.common cimport LogColor
from nautilus_trader.core.rust.common cimport LogLevel
from nautilus_trader.core.rust.common cimport log_color_from_cstr
from nautilus_trader.core.rust.common cimport log_color_to_cstr
from nautilus_trader.core.rust.common cimport log_level_from_cstr
from nautilus_trader.core.rust.common cimport log_level_to_cstr
from nautilus_trader.core.rust.common cimport logger_flush
from nautilus_trader.core.rust.common cimport logger_log
from nautilus_trader.core.rust.common cimport logging_init
from nautilus_trader.core.rust.common cimport tracing_init
from nautilus_trader.core.string cimport cstr_to_pystr
from nautilus_trader.core.string cimport pybytes_to_cstr
from nautilus_trader.core.string cimport pystr_to_cstr
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.model.identifiers cimport TraderId


# TODO!: Reimplementing logging config
# from nautilus_trader.core import nautilus_pyo3
# from nautilus_trader.core.nautilus_pyo3 import logger_log



cpdef void init_tracing():
    tracing_init()


cpdef void init_logging(
    TraderId trader_id,
    UUID4 instance_id,
    str config_spec,
    str directory,
    str file_name,
    str file_format,
):
    Condition.valid_string(config_spec, "config_spec")

    logging_init(
        trader_id._mem,
        instance_id._mem,
        pystr_to_cstr(config_spec),
        pystr_to_cstr(directory) if directory else NULL,
        pystr_to_cstr(file_name) if file_name else NULL,
        pystr_to_cstr(file_format) if file_format else NULL,
    )


cpdef LogColor log_color_from_str(str value):
    return log_color_from_cstr(pystr_to_cstr(value))


cpdef str log_color_to_str(LogColor value):
    return cstr_to_pystr(log_color_to_cstr(value))


cpdef LogLevel log_level_from_str(str value):
    return log_level_from_cstr(pystr_to_cstr(value))


cpdef str log_level_to_str(LogLevel value):
    return cstr_to_pystr(log_level_to_cstr(value))


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
    level_stdout : LogLevel, default ``INFO``
        The minimum log level to write to stdout.
    level_file : LogLevel, default ``DEBUG``
        The minimum log level to write to a file.
    file_logging : bool, default False
        If logging to a file is enabled.
    directory : str, optional
        The path to the log file directory.
        If ``None`` then will write to the current working directory.
    file_name : str, optional
        The custom log file name (will use a '.log' suffix for plain text or '.json' for JSON).
        If ``None`` will not log to a file (unless `file_auto` is True).
    file_format : str { 'JSON' }, optional
        The log file format. If ``None`` (default) then will log in plain text.
        If set to 'JSON' then logs will be in JSON format.
    component_levels : dict[ComponentId, LogLevel]
        The additional per component log level filters, where keys are component
        IDs (e.g. actor/strategy IDs) and values are log levels.
    colors : bool, default True
        If ANSI codes should be used to produce colored log lines.
    bypass : bool, default False
        If the log output is bypassed.
    """

    def __init__(
        self,
        Clock clock not None,
        TraderId trader_id = None,
        str machine_id = None,
        UUID4 instance_id = None,
        LogLevel level_stdout = LogLevel.INFO,
        LogLevel level_file = LogLevel.DEBUG,
        bint file_logging = False,
        str directory = None,
        str file_name = None,
        str file_format = None,
        dict component_levels: dict[ComponentId, LogLevel] = None,
        bint colors = True,
        bint bypass = False,
    ):
        if trader_id is None:
            trader_id = TraderId("TRADER-000")
        if instance_id is None:
            instance_id = UUID4()
        if machine_id is None:
            machine_id = socket.gethostname()

        self._clock = clock
        self._trader_id = trader_id
        self._instance_id = instance_id
        self._machine_id = machine_id
        self._is_bypassed = bypass
        self._is_colored = colors

    @property
    def trader_id(self) -> TraderId:
        """
        Return the loggers trader ID.

        Returns
        -------
        TraderId

        """
        return self._trader_id

    @property
    def machine_id(self) -> str:
        """
        Return the loggers machine ID.

        Returns
        -------
        str

        """
        return self._machine_id

    @property
    def instance_id(self) -> UUID4:
        """
        Return the loggers system instance ID.

        Returns
        -------
        UUID4

        """
        return self._instance_id

    @property
    def is_colored(self) -> bool:
        """
        Return whether the logger is using ANSI color codes.

        Returns
        -------
        bool

        """
        return self._is_colored

    @property
    def is_bypassed(self) -> bool:
        """
        Return whether the logger is in bypass mode.

        Returns
        -------
        bool

        """
        return self._is_bypassed

    cdef void log(
        self,
        LogLevel level,
        LogColor color,
        const char* component_cstr,
        str message,
    ):
        logger_log(
            self._clock.timestamp_ns(),
            level,
            color,
            component_cstr,
            pystr_to_cstr(message),
        )

    # TODO!: Reimplementing logging config
    # cdef void log(
    #     self,
    #     level,
    #     color,
    #     str component,
    #     str message,
    # ):
    #     logger_log(
    #         self._clock.timestamp_ns(),
    #         level,
    #         nautilus_pyo3.LogColor.Normal,  # In development
    #         component,
    #         message,
    #     )

    cpdef void change_clock(self, Clock clock):
        """
        Change the loggers internal clock to the given clock.

        Parameters
        ----------
        clock : Clock

        """
        Condition.not_none(clock, "clock")

        self._clock = clock

    cpdef void flush(self):
        """
        Flush all logger buiffers.

        """
        logger_flush()


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
        self._component_cstr = pystr_to_cstr(component_name)
        self._is_colored = logger.is_colored
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
    def is_colored(self) -> bool:
        """
        Return whether the logger is using ANSI color codes.

        Returns
        -------
        bool

        """
        return self._is_colored

    @property
    def is_bypassed(self) -> bool:
        """
        Return whether the logger is in bypass mode.

        Returns
        -------
        bool

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
        str message,
        LogColor color = LogColor.NORMAL,
        dict annotations = None,
    ):
        """
        Log the given debug message with the logger.

        Parameters
        ----------
        message : str
            The log message content.
        color : LogColor, optional
            The log message color.

        """
        Condition.not_none(message, "message")

        if self.is_bypassed:
            return

        self._logger.log(
            LogLevel.DEBUG,
            color,
            self._component_cstr,
            message,
        )

        # TODO!: Reimplementing logging config
        # self._logger.log(
        #     nautilus_pyo3.LogLevel.Debug,
        #     color,
        #     self._component,
        #     message,
        # )

    cpdef void info(
        self, str message,
        LogColor color = LogColor.NORMAL,
        dict annotations = None,
    ):
        """
        Log the given information message with the logger.

        Parameters
        ----------
        message : str
            The log message content.
        color : LogColor, optional
            The log message color.
        annotations : dict[str, object], optional
            The annotations for the log record.

        """
        Condition.not_none(message, "message")

        if self.is_bypassed:
            return

        self._logger.log(
            LogLevel.INFO,
            color,
            self._component_cstr,
            message,
        )

        # TODO!: Reimplementing logging config
        # self._logger.log(
        #     nautilus_pyo3.LogLevel.Info,
        #     color,
        #     self._component,
        #     message,
        # )

    cpdef void warning(
        self,
        str message,
        LogColor color = LogColor.YELLOW,
        dict annotations = None,
    ):
        """
        Log the given warning message with the logger.

        Parameters
        ----------
        message : str
            The log message content.
        color : LogColor, optional
            The log message color.
        annotations : dict[str, object], optional
            The annotations for the log record.

        """
        Condition.not_none(message, "message")

        if self.is_bypassed:
            return

        self._logger.log(
            LogLevel.WARNING,
            color,
            self._component_cstr,
            message,
        )

        # TODO!: Reimplementing logging config
        # self._logger.log(
        #     nautilus_pyo3.LogLevel.Warning,
        #     color,
        #     self._component,
        #     message,
        # )

    cpdef void error(
        self,
        str message,
        LogColor color = LogColor.RED,
        dict annotations = None,
    ):
        """
        Log the given error message with the logger.

        Parameters
        ----------
        message : str
            The log message content.
        color : LogColor, optional
            The log message color.
        annotations : dict[str, object], optional
            The annotations for the log record.

        """
        Condition.not_none(message, "message")

        if self.is_bypassed:
            return

        self._logger.log(
            LogLevel.ERROR,
            color,
            self._component_cstr,
            message,
        )

        # TODO!: Reimplementing logging config
        # self._logger.log(
        #     nautilus_pyo3.LogLevel.Error,
        #     color,
        #     self._component,
        #     message,
        # )

    cpdef void exception(
        self,
        str message,
        ex,
        dict annotations = None,
    ):
        """
        Log the given exception including stack trace information.

        Parameters
        ----------
        message : str
            The log message content.
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

        self.error(f"{message}\n{ex_string}\n{stack_trace_lines}", annotations=annotations)


cpdef void nautilus_header(LoggerAdapter logger):
    Condition.not_none(logger, "logger")

    color = "\033[36m" if logger.is_colored else ""

    print("")  # New line to begin
    logger.info(f"{color}=================================================================")
    logger.info(f"{color} NAUTILUS TRADER - Automated Algorithmic Trading Platform")
    logger.info(f"{color} by Nautech Systems Pty Ltd.")
    logger.info(f"{color} Copyright (C) 2015-2024. All rights reserved.")
    logger.info(f"{color}=================================================================")
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
    logger.info(f"{color}=================================================================")
    logger.info(f"{color} SYSTEM SPECIFICATION")
    logger.info(f"{color}=================================================================")
    logger.info(f"CPU architecture: {platform.processor()}")
    try:
        cpu_freq_str = f"@ {int(psutil.cpu_freq()[2])} MHz"
    except Exception:  # noqa (historically problematic call on ARM)
        cpu_freq_str = None
    logger.info(f"CPU(s): {psutil.cpu_count()} {cpu_freq_str}")
    logger.info(f"OS: {platform.platform()}")
    log_memory(logger)
    logger.info(f"{color}=================================================================")
    logger.info(f"{color} IDENTIFIERS")
    logger.info(f"{color}=================================================================")
    logger.info(f"trader_id: {logger.trader_id}")
    logger.info(f"machine_id: {logger.machine_id}")
    logger.info(f"instance_id: {logger.instance_id}")
    logger.info(f"{color}=================================================================")
    logger.info(f"{color} VERSIONING")
    logger.info(f"{color}=================================================================")
    logger.info(f"nautilus-trader {__version__}")
    logger.info(f"python {python_version()}")
    logger.info(f"numpy {np.__version__}")
    logger.info(f"pandas {pd.__version__}")
    logger.info(f"msgspec {msgspec.__version__}")
    logger.info(f"psutil {psutil.__version__}")
    logger.info(f"pyarrow {pyarrow.__version__}")
    logger.info(f"pytz {pytz.__version__}")  # type: ignore
    try:
        import uvloop
        logger.info(f"uvloop {uvloop.__version__}")
    except ImportError:  # pragma: no cover
        uvloop = None

    logger.info(f"{color}=================================================================")

cpdef void log_memory(LoggerAdapter logger):
    Condition.not_none(logger, "logger")

    color = "\033[36m" if logger.is_colored else ""

    logger.info(f"{color}=================================================================")
    logger.info(f"{color} MEMORY USAGE")
    logger.info(f"{color}=================================================================")
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
