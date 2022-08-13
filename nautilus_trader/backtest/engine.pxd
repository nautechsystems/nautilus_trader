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

from cpython.datetime cimport datetime
from libc.stdint cimport uint64_t

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.core.data cimport Data
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.system.kernel cimport NautilusKernel


cdef class BacktestEngine:
    cdef object _config
    cdef Clock _clock

    cdef readonly LoggerAdapter _log
    cdef Logger _logger

    cdef dict _venues
    cdef list _data
    cdef uint64_t _data_len
    cdef uint64_t _index

    cdef readonly NautilusKernel kernel
    """The internal kernel for the engine.\n\n:returns: `NautilusKernel`"""
    cdef readonly str run_config_id
    """The last backtest engine run config ID.\n\n:returns: `str` or ``None``"""
    cdef readonly UUID4 run_id
    """The last backtest engine run ID (if run).\n\n:returns: `UUID4` or ``None``"""
    cdef readonly int iteration
    """The backtest engine iteration count.\n\n:returns: `int`"""
    cdef readonly datetime run_started
    """When the last backtest run started (if run).\n\n:returns: `datetime` or ``None``"""
    cdef readonly datetime run_finished
    """When the last backtest run finished (if run).\n\n:returns: `datetime` or ``None``"""
    cdef readonly datetime backtest_start
    """The last backtest run time range start (if run).\n\n:returns: `datetime` or ``None``"""
    cdef readonly datetime backtest_end
    """The last backtest run time range end (if run).\n\n:returns: `datetime` or ``None``"""

    cdef Data _next(self)
    cdef list _advance_time(self, uint64_t now_ns)
