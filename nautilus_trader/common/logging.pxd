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

from typing import Callable

from libc.stdint cimport uint64_t

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.rust.common cimport LogColor
from nautilus_trader.core.rust.common cimport LogLevel
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.model.identifiers cimport TraderId


cdef str RECV
cdef str SENT
cdef str CMD
cdef str EVT
cdef str DOC
cdef str RPT
cdef str REQ
cdef str RES

cpdef bint is_logging_initialized()
cpdef void set_logging_clock_realtime()
cpdef void set_logging_clock_static(uint64_t time_ns)

cpdef LogColor log_color_from_str(str value)
cpdef str log_color_to_str(LogColor value)

cpdef LogLevel log_level_from_str(str value)
cpdef str log_level_to_str(LogLevel value)

cpdef void init_tracing()

cpdef void init_logging(
    TraderId trader_id=*,
    str machine_id=*,
    UUID4 instance_id=*,
    LogLevel level_stdout=*,
    LogLevel level_file=*,
    str directory=*,
    str file_name=*,
    str file_format=*,
    dict component_levels=*,
    bint colors=*,
    bint bypass=*,
    bint print_config=*,
)

cpdef void shutdown_logging()


cdef class Logger:
    cdef str _name

    cpdef void flush(self)
    cpdef void debug(self, str message, LogColor color=*)
    cpdef void info(self, str message, LogColor color=*)
    cpdef void warning(self, str message, LogColor color=*)
    cpdef void error(self, str message, LogColor color=*)
    cpdef void exception(self, str message, ex)


cpdef void log_nautilus_header(
    Logger logger,
    TraderId trader_id,
    str machine_id,
    UUID4 instance_id,
    bint is_colored,
)

cpdef void log_memory(Logger logger, bint is_colored)
