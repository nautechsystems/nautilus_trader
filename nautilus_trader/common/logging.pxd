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

from typing import Callable

from libc.stdint cimport uint64_t

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.rust.common cimport LogColor
from nautilus_trader.core.rust.common cimport Logger_API
from nautilus_trader.core.rust.common cimport LogLevel


cpdef LogColor log_color_from_str(str value)
cpdef str log_color_to_str(LogColor value)

cpdef LogLevel log_level_from_str(str value)
cpdef str log_level_to_str(LogLevel value)


cdef str RECV
cdef str SENT
cdef str CMD
cdef str EVT
cdef str DOC
cdef str RPT
cdef str REQ
cdef str RES


cdef class Logger:
    cdef Logger_API _mem
    cdef Clock _clock

    cpdef void change_clock(self, Clock clock)
    cdef void log(
        self,
        uint64_t timestamp,
        LogLevel level,
        LogColor color,
        str component,
        str message,
        dict annotations=*,
    )
    cdef void _log(
        self,
        uint64_t timestamp,
        LogLevel level,
        LogColor color,
        str component,
        str message,
        dict annotations,
    )


cdef class LoggerAdapter:
    cdef Logger _logger
    cdef str _component
    cdef bint _is_colored
    cdef bint _is_bypassed

    cpdef Logger get_logger(self)
    cpdef void debug(self, str message, LogColor color=*, dict annotations=*)
    cpdef void info(self, str message, LogColor color=*, dict annotations=*)
    cpdef void warning(self, str message, LogColor color=*, dict annotations=*)
    cpdef void error(self, str message, LogColor color=*, dict annotations=*)
    cpdef void critical(self, str message, LogColor color=*, dict annotations=*)
    cpdef void exception(self, str message, ex, dict annotations=*)


cpdef void nautilus_header(LoggerAdapter logger)
cpdef void log_memory(LoggerAdapter logger)
