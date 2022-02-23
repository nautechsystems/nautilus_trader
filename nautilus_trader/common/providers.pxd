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

from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instruments.base cimport Instrument


cdef class InstrumentProvider:
    cdef dict _instruments
    cdef dict _currencies

    cdef bint _load_all_on_start
    cdef set _load_ids_on_start

    cdef bint _loaded
    cdef bint _loading

    cdef readonly LoggerAdapter _log
    cdef readonly object _filters
    cdef readonly Venue venue
    """The providers venue.\n\n:returns: `Venue`"""

    cpdef void add_currency(self, Currency currency) except *
    cpdef void add(self, Instrument instrument) except *
    cpdef void add_bulk(self, list instruments) except *
    cpdef list list_all(self)
    cpdef dict get_all(self)
    cpdef dict currencies(self)
    cpdef Currency currency(self, str code)
    cpdef Instrument find(self, InstrumentId instrument_id)
