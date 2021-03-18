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

from nautilus_trader.common.providers cimport InstrumentProvider
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.instrument cimport Instrument

cdef class BetfairInstrumentProvider(InstrumentProvider):
    cdef object _client
    cdef object _currencies

    cdef readonly venue

    cpdef Currency currency(self, str code)
    cdef void _load_instruments(self) except *
    cdef void _load_currencies(self) except *
    cdef Instrument _parse_instrument(self, InstrumentId instrument_id, dict values)
