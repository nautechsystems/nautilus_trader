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
import time
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
from nautilus_trader.common.clock cimport LiveClock
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
from nautilus_trader.core.rust.common cimport logging_clock_set_realtime
from nautilus_trader.core.rust.common cimport logging_clock_set_static
from nautilus_trader.core.rust.common cimport logging_init
from nautilus_trader.core.rust.common cimport logging_is_colored
from nautilus_trader.core.rust.common cimport logging_is_initialized
from nautilus_trader.core.rust.common cimport logging_log_header
from nautilus_trader.core.rust.common cimport logging_log_sysinfo
from nautilus_trader.core.rust.common cimport logging_shutdown
from nautilus_trader.core.rust.common cimport tracing_init
from nautilus_trader.core.string cimport cstr_to_pystr
from nautilus_trader.core.string cimport pybytes_to_cstr
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


cpdef bint is_logging_initialized():
    return <bint>logging_is_initialized()


cpdef void set_logging_clock_realtime():
    logging_clock_set_realtime()


cpdef void set_logging_clock_static(uint64_t time_ns):
    logging_clock_set_static(time_ns)


cpdef LogColor log_color_from_str(str value):
    return log_color_from_cstr(pystr_to_cstr(value))


cpdef str log_color_to_str(LogColor value):
    return cstr_to_pystr(log_color_to_cstr(value))


cpdef LogLevel log_level_from_str(str value):
    return log_level_from_cstr(pystr_to_cstr(value))


cpdef str log_level_to_str(LogLevel value):
    return cstr_to_pystr(log_level_to_cstr(value))


cpdef void init_tracing():
    """
    Initialize tracing for async Rust.

    """
    tracing_init()


cpdef void init_logging(
    TraderId trader_id = None,
    str machine_id = None,
    UUID4 instance_id = None,
    LogLevel level_stdout = LogLevel.INFO,
    LogLevel level_file = LogLevel.OFF,
    str directory = None,
    str file_name = None,
    str file_format = None,
    dict component_levels: dict[ComponentId, LogLevel] = None,
    bint colors = True,
    bint bypass = False,
    bint print_config = False,
):
    """
    Initialize the logging system.

    Acts as an interface into the logging system implemented in Rust with the `log` crate.

    This function should only be called once per process, at the beginning of the application
    run.

    Parameters
    ----------
    trader_id : TraderId, optional
        The trader ID for the logger.
    machine_id : str, optional
        The machine ID.
    instance_id : UUID4, optional
        The instance ID.
    level_stdout : LogLevel, default ``INFO``
        The minimum log level to write to stdout.
    level_file : LogLevel, default ``OFF``
        The minimum log level to write to a file.
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
        If the output for the core logging system is bypassed (useful for logging tests).
    print_config : bool, default False
        If the core logging configuration should be printed to stdout on initialization.

    """
    if trader_id is None:
        trader_id = TraderId("TRADER-000")
    if machine_id is None:
        machine_id = socket.gethostname()
    if instance_id is None:
        instance_id = UUID4()

    if not logging_is_initialized():
        logging_init(
            trader_id._mem,
            instance_id._mem,
            level_stdout,
            level_file,
            pystr_to_cstr(directory) if directory else NULL,
            pystr_to_cstr(file_name) if file_name else NULL,
            pystr_to_cstr(file_format) if file_format else NULL,
            pybytes_to_cstr(msgspec.json.encode(component_levels)) if component_levels else NULL,
            colors,
            bypass,
            print_config,
        )


cpdef void shutdown_logging():
    if logging_is_initialized():
        logging_shutdown()


cdef class Logger:
    """
    Provides a logger adapter into the logging system.

    Parameters
    ----------
    name : str
        The name of the logger. This will appear within each log line.

    """

    def __init__(self, str name not None) -> None:
        Condition.valid_string(name, "name")

        self._name = name

    cpdef void flush(self):
        """
        Flush all buffers for the logging system.

        This could include stdout/stderr and file writer buffers.

        Warning
        -------
        This method is intended to be called once at application shutdown.
        It will intentionally block the main thread for 100 milliseconds, allowing all
        buffers to be flushed prior to exiting.

        """
        if logging_is_initialized():
            logger_flush()
            time.sleep(0.1)  # Temporary solution before joining logging thread

    cpdef void debug(
        self,
        str message,
        LogColor color = LogColor.NORMAL,
    ):
        """
        Log the given debug level message.

        Parameters
        ----------
        message : str
            The log message text (valid UTF-8).
        color : LogColor, optional
            The log message color.

        """
        if not logging_is_initialized():
            return

        logger_log(
            LogLevel.DEBUG,
            color,
            pystr_to_cstr(self._name),  # TODO: Optimize this
            pystr_to_cstr(message) if message is not None else NULL,
        )

    cpdef void info(
        self, str message,
        LogColor color = LogColor.NORMAL,
    ):
        """
        Log the given information level message.

        Parameters
        ----------
        message : str
            The log message text (valid UTF-8).
        color : LogColor, optional
            The log message color.

        """
        if not logging_is_initialized():
            return

        logger_log(
            LogLevel.INFO,
            color,
            pystr_to_cstr(self._name),  # TODO: Optimize this
            pystr_to_cstr(message) if message is not None else NULL,
        )

    cpdef void warning(
        self,
        str message,
        LogColor color = LogColor.YELLOW,
    ):
        """
        Log the given warning level message.

        Parameters
        ----------
        message : str
            The log message text (valid UTF-8).
        color : LogColor, optional
            The log message color.

        """
        if not logging_is_initialized():
            return

        logger_log(
            LogLevel.WARNING,
            color,
            pystr_to_cstr(self._name),  # TODO: Optimize this
            pystr_to_cstr(message) if message is not None else NULL,
        )

    cpdef void error(
        self,
        str message,
        LogColor color = LogColor.RED,
    ):
        """
        Log the given error level message.

        Parameters
        ----------
        message : str
            The log message text (valid UTF-8).
        color : LogColor, optional
            The log message color.

        """
        if not logging_is_initialized():
            return

        logger_log(
            LogLevel.ERROR,
            color,
            pystr_to_cstr(self._name),  # TODO: Optimize this
            pystr_to_cstr(message) if message is not None else NULL,
        )

    cpdef void exception(
        self,
        str message,
        ex,
    ):
        """
        Log the given exception including stack trace information.

        Parameters
        ----------
        message : str
            The log message text (valid UTF-8).
        ex : Exception
            The exception to log.

        """
        Condition.not_none(ex, "ex")

        cdef str ex_string = f"{type(ex).__name__}({ex})"
        ex_type, ex_value, ex_traceback = sys.exc_info()
        stack_trace = traceback.format_exception(ex_type, ex_value, ex_traceback)

        cdef str stack_trace_lines = ""
        cdef str line
        for line in stack_trace[:len(stack_trace) - 1]:
            stack_trace_lines += line

        self.error(f"{message}\n{ex_string}\n{stack_trace_lines}")


cpdef void log_header(
    TraderId trader_id,
    str machine_id,
    UUID4 instance_id,
    str component,
):
    logging_log_header(
        trader_id._mem,
        pystr_to_cstr(machine_id),
        instance_id._mem,
        pystr_to_cstr(component),
    )


cpdef void log_sysinfo(
    TraderId trader_id,
    str machine_id,
    UUID4 instance_id,
    str component,
):
    logging_log_sysinfo(
        pystr_to_cstr(component),
    )
