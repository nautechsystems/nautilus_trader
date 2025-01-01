# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.backtest.exchange cimport SimulatedExchange
from nautilus_trader.common.component cimport Clock
from nautilus_trader.common.component cimport Logger
from nautilus_trader.core.data cimport Data
from nautilus_trader.core.rust.backtest cimport TimeEventAccumulatorAPI
from nautilus_trader.core.rust.core cimport CVec
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.data.engine cimport DataEngine


cdef class BacktestEngine:
    cdef object _config
    cdef Clock _clock
    cdef Logger _log
    cdef TimeEventAccumulatorAPI _accumulator

    cdef object _kernel
    cdef UUID4 _instance_id
    cdef DataEngine _data_engine
    cdef str _run_config_id
    cdef UUID4 _run_id
    cdef datetime _run_started
    cdef datetime _run_finished
    cdef datetime _backtest_start
    cdef datetime _backtest_end

    cdef dict[Venue, SimulatedExchange] _venues
    cdef set[InstrumentId] _has_data
    cdef set[InstrumentId] _has_book_data
    cdef list[Data] _data
    cdef uint64_t _data_len
    cdef uint64_t _index
    cdef uint64_t _iteration

    cdef Data _next(self)
    cdef CVec _advance_time(self, uint64_t ts_now)
    cdef void _process_raw_time_event_handlers(
        self,
        CVec raw_handlers,
        uint64_t ts_now,
        bint only_now,
        bint asof_now=*,
    )


cdef inline bint should_skip_time_event(
    uint64_t ts_event_init,
    uint64_t ts_now,
    bint only_now,
    bint asof_now,
):
    if only_now and ts_event_init < ts_now:
        return True
    if (not only_now) and (ts_event_init == ts_now):
        return True
    if asof_now and ts_event_init > ts_now:
        return True

    return False
