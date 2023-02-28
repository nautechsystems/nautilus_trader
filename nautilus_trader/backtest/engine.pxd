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

from cpython.datetime cimport datetime
from libc.stdint cimport uint64_t

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.core.data cimport Data
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.data.engine cimport DataEngine


cdef class BacktestEngine:
    cdef object _config
    cdef Clock _clock

    cdef readonly LoggerAdapter _log
    cdef Logger _logger

    cdef object _kernel
    cdef DataEngine _data_engine
    cdef str _run_config_id
    cdef UUID4 _run_id
    cdef datetime _run_started
    cdef datetime _run_finished
    cdef datetime _backtest_start
    cdef datetime _backtest_end

    cdef dict _venues
    cdef list _data
    cdef uint64_t _data_len
    cdef uint64_t _index
    cdef uint64_t _iteration

    cdef Data _next(self)
    cdef list _advance_time(self, uint64_t now_ns, list clocks)
