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

from cpython.datetime cimport datetime

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport LogMessage
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.serialization.base cimport LogSerializer


cdef str RECV
cdef str SENT
cdef str CMD
cdef str EVT


cpdef enum LogLevel:
    UNDEFINED = 0,  # Invalid value
    VERBOSE = 1,
    DEBUG = 2,
    INFO = 3,
    WARNING = 4,
    ERROR = 5,
    CRITICAL = 6,
    FATAL = 7,


cdef inline str log_level_to_string(int value):
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


cdef inline LogLevel log_level_from_string(str value):
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


cdef class LogMessage:
    cdef readonly datetime timestamp
    cdef readonly LogLevel level
    cdef readonly str text
    cdef readonly long thread_id
    cdef str level_string(self)
    cdef str as_string(self)


cdef class Logger:
    cdef LogLevel _log_level_console
    cdef LogLevel _log_level_file
    cdef LogLevel _log_level_store
    cdef bint _console_prints
    cdef bint _log_thread
    cdef bint _log_to_file
    cdef str _log_file_path
    cdef str _log_file
    cdef list _log_store
    cdef object _log_file_handler
    cdef object _logger

    cdef readonly str name
    cdef readonly bint bypass_logging
    cdef readonly Clock clock

    cpdef void change_log_file_name(self, str name) except *
    cpdef void log(self, LogMessage message) except *
    cpdef list get_log_store(self)
    cpdef void clear_log_store(self) except *
    cpdef void _log(self, LogMessage message) except *
    cdef str _format_output(self, LogMessage message)
    cdef void _in_memory_log_store(self, LogLevel level, str text) except *
    cdef void _print_to_console(self, LogLevel level, str text) except *


cdef class LoggerAdapter:
    cdef Logger _logger

    cdef readonly bint bypassed
    cdef readonly str component_name

    cpdef Logger get_logger(self)
    cpdef void verbose(self, str message) except *
    cpdef void debug(self, str message) except *
    cpdef void info(self, str message) except *
    cpdef void warning(self, str message) except *
    cpdef void error(self, str message) except *
    cpdef void critical(self, str message) except *
    cpdef void exception(self, ex) except *
    cdef void _send_to_logger(self, LogLevel level, str message) except *
    cdef str _format_message(self, str message)


cpdef void nautilus_header(LoggerAdapter logger) except *


cdef class LogStore:
    cdef str _key
    cdef LogSerializer _serializer

    cpdef void store(self, LogMessage message)
    cpdef void _consume_messages(self) except *


cdef class LiveLogger(Logger):
    cdef object _queue
    cdef object _thread
    cdef LogStore _store

    cpdef void _consume_messages(self) except *
