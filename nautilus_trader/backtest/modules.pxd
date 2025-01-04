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

from nautilus_trader.accounting.calculators cimport RolloverInterestCalculator
from nautilus_trader.backtest.exchange cimport SimulatedExchange
from nautilus_trader.common.actor cimport Actor
from nautilus_trader.common.component cimport Logger
from nautilus_trader.core.data cimport Data


cdef class SimulationModule(Actor):
    cdef readonly SimulatedExchange exchange

    cpdef void register_venue(self, SimulatedExchange exchange)
    cpdef void pre_process(self, Data data)
    cpdef void process(self, uint64_t ts_now)
    cpdef void log_diagnostics(self, Logger logger)
    cpdef void reset(self)


cdef class FXRolloverInterestModule(SimulationModule):
    cdef RolloverInterestCalculator _calculator
    cdef object _rollover_spread
    cdef datetime _rollover_time
    cdef bint _rollover_applied
    cdef dict _rollover_totals
    cdef int _day_number

    cdef void _apply_rollover_interest(self, datetime timestamp, int iso_week_day)
