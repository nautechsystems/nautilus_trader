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

from libc.stdint cimport uint64_t

from nautilus_trader.backtest.modules cimport SimulationModule
from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.core.data cimport Data
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.instruments.crypto_option cimport CryptoOption
from nautilus_trader.model.instruments.option_contract cimport OptionContract
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.position cimport Position


cdef class OptionExerciseModule(SimulationModule):
    cdef public object config
    cdef public Cache cache
    cdef public dict expiry_timers
    cdef public set processed_expiries

    cpdef void pre_process(self, Data data)
    cpdef Instrument _get_underlying_instrument(self, object option)
