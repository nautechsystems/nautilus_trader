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

from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.providers cimport InstrumentProvider
from nautilus_trader.model.instruments.betting cimport BettingInstrument


cdef class BetfairInstrumentProvider(InstrumentProvider):
    cdef object _client
    cdef LoggerAdapter _log
    cdef dict market_filter
    cdef dict _cache
    cdef set _searched_filters
    cdef str _account_currency

    cdef readonly venue

    cdef void _load_instruments(self, dict market_filter=*) except *
    cpdef void _assert_loaded_instruments(self) except *
    cpdef list search_markets(self, dict market_filter=*)
    cpdef list search_instruments(self, dict instrument_filter=*, bint load=*)
    cpdef list list_instruments(self)
    cpdef BettingInstrument get_betting_instrument(self, str market_id, str selection_id, str handicap)
    cpdef str get_account_currency(self)
    cpdef void set_instruments(self, list instruments) except *
    cpdef void add_instruments(self, list instruments) except *
