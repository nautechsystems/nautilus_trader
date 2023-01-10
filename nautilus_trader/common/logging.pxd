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

from cpython.datetime cimport datetime
from cpython.datetime cimport timedelta
from libc.stdint cimport uint64_t

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.queue cimport Queue
from nautilus_trader.core.rust.common cimport CLogger
from nautilus_trader.core.rust.common cimport LogColor
from nautilus_trader.core.rust.common cimport LogLevel


cdef str RECV
cdef str SENT
cdef str CMD
cdef str EVT
cdef str DOC
cdef str RPT
cdef str REQ
cdef str RES


cdef class Logger:
    cdef CLogger _mem
    cdef Clock _clock
    cdef list _sinks

    cpdef void register_sink(self, handler: Callable[[dict], None]) except *
    cpdef void change_clock(self, Clock clock) except *
    cdef dict create_record(self, LogLevel level, str component, str msg, dict annotations=*)
    cdef void log(
        self,
        uint64_t timestamp_ns,
        LogLevel level,
        LogColor color,
        str component,
        str msg,
        dict annotations=*,
    ) except *
    cdef void _log(
        self,
        uint64_t timestamp_ns,
        LogLevel level,
        LogColor color,
        str component,
        str msg,
        dict annotations,
    ) except *


cdef class LoggerAdapter:
    cdef Logger _logger
    cdef str _component
    cdef bint _is_bypassed

    cpdef Logger get_logger(self)
    cpdef void debug(self, str msg, LogColor color=*, dict annotations=*) except *
    cpdef void info(self, str msg, LogColor color=*, dict annotations=*) except *
    cpdef void warning(self, str msg, LogColor color=*, dict annotations=*) except *
    cpdef void error(self, str msg, LogColor color=*, dict annotations=*) except *
    cpdef void critical(self, str msg, LogColor color=*, dict annotations=*) except *
    cpdef void exception(self, str msg, ex, dict annotations=*) except *


cpdef void nautilus_header(LoggerAdapter logger) except *
cpdef void log_memory(LoggerAdapter logger) except *


cdef class LiveLogger(Logger):
    cdef object _loop
    cdef object _run_task
    cdef timedelta _blocked_log_interval
    cdef Queue _queue
    cdef bint _is_running
    cdef datetime _last_blocked

    cpdef void start(self) except *
    cpdef void stop(self) except *
    cdef void _enqueue_sentinel(self) except *
