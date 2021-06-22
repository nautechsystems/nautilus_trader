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
from nautilus_trader.model.c_enums.asset_class cimport AssetClass
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.instruments.equity cimport Equity
from nautilus_trader.model.instruments.future cimport Future


cdef class IBInstrumentProvider(InstrumentProvider):
    cdef object _client
    cdef str _host
    cdef str _port
    cdef int _client_id

    cpdef void connect(self)
    cdef int _tick_size_to_precision(self, double tick_size) except *
    cdef Future _parse_futures_contract(self, InstrumentId instrument_id, AssetClass asset_class, list details_list)
    cpdef Equity retrieve_equity_contract(self, str name, str exchange, str currency)
