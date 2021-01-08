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

from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instrument cimport Instrument


cdef class BinanceInstrumentProvider:
    cdef dict _instruments
    cdef object _client

    cdef readonly Venue venue
    """The venue of the provider.\n\n:returns: `Venue`"""
    cdef readonly int count
    """The count of instruments held by the provider.\n\n:returns: `int`"""

    cpdef void load_all(self) except *
    cpdef dict get_all(self)
    cpdef Instrument get(self, Symbol symbol)

    cdef void _load_instruments(self) except *
    cdef Instrument _parse_instrument(self, Symbol symbol, dict values)
