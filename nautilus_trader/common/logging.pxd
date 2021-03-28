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

from cpython.datetime cimport datetime
from libc.stdint cimport int64_t

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport LogMessage
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.queue cimport Queue


cdef str RECV
cdef str SENT
cdef str CMD
cdef str EVT
cdef str REQ
cdef str RES


cpdef enum LogLevel:
    VERBOSE = 1,
    DEBUG = 2,
    INFO = 3,
    WARNING = 4,
    ERROR = 5,
    CRITICAL = 6,
    FATAL = 7,


cpdef enum LogColor:
    NORMAL = 0,
    GREEN = 1,
    BLUE = 2,
    YELLOW = 3,
    RED = 4,


cdef class LogLevelParser:

    @staticmethod
    cdef str to_str(int value)

    @staticmethod
    cdef LogLevel from_str(str value)


cdef class LogMessage:
    cdef readonly datetime timestamp
    """The log message timestamp.\n\n:returns: `datetime`"""
    cdef readonly LogLevel level
    """The log level.\n\n:returns: `LogLevel` (Enum)"""
    cdef readonly LogColor color
    """The log text color.\n\n:returns: `LogColor` (Enum)"""
    cdef readonly str text
    """The log text.\n\n:returns: `str`"""
    cdef readonly int64_t thread_id
    """The thread identifier.\n\n:returns: `int64`"""

    cdef inline str as_string(self)


cdef class Logger:
    cdef LogLevel _log_level_console
    cdef LogLevel _log_level_file
    cdef LogLevel _log_level_store
    cdef bint _console_prints
    cdef bint _log_thread
    cdef bint _log_to_file
    cdef str _log_file_dir
    cdef str _log_file_path
    cdef list _log_store
    cdef object _log_file_handler
    cdef object _logger

    cdef readonly str name
    """The loggers name.\n\n:returns: `str`"""
    cdef readonly bint bypass_logging
    """If the logger is in bypass mode.\n\n:returns: `bool`"""
    cdef readonly Clock clock
    """The loggers clock.\n\n:returns: `Clock`"""

    cpdef str get_log_file_dir(self)
    cpdef str get_log_file_path(self)
    cpdef list get_log_store(self)
    cpdef void change_log_file_name(self, str name) except *
    cpdef void log(self, LogMessage message) except *
    cpdef void clear_log_store(self) except *

    cpdef void _log(self, LogMessage message) except *
    cdef str _format_output(self, LogMessage message)
    cdef void _in_memory_log_store(self, LogLevel level, str text) except *
    cdef void _print_to_console(self, LogLevel level, str text) except *


cdef class LoggerAdapter:
    cdef Logger _logger

    cdef readonly str component_name
    """The loggers component name.\n\n:returns: `str`"""
    cdef readonly bint is_bypassed
    """If the logger is in bypass mode.\n\n:returns: `bool`"""

    cpdef Logger get_logger(self)
    cpdef void verbose(self, str message, LogColor color=*) except *
    cpdef void debug(self, str message, LogColor color=*) except *
    cpdef void info(self, str message, LogColor color=*) except *
    cpdef void warning(self, str message) except *
    cpdef void error(self, str message) except *
    cpdef void critical(self, str message) except *
    cpdef void exception(self, ex) except *
    cdef inline void _send_to_logger(self, LogLevel level, LogColor color, str message) except *
    cdef inline str _format_message(self, str message)


cpdef void nautilus_header(LoggerAdapter logger) except *
cpdef void log_memory(LoggerAdapter logger) except *


cdef class TestLogger(Logger):
    pass


cdef class LiveLogger(Logger):
    cdef object _loop
    cdef object _run_task
    cdef Queue _queue

    cdef readonly bint is_running
    """If the logger is running an event loop task.\n\n:returns: `bool`"""

    cpdef void start(self) except *
    cpdef void stop(self) except *
