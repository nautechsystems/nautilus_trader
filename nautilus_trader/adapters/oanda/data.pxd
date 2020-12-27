# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.adapters.oanda.providers cimport OandaInstrumentProvider
from nautilus_trader.live.data cimport LiveDataClient
from nautilus_trader.model.bar cimport Bar
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.instrument cimport Instrument


cdef class OandaDataClient(LiveDataClient):
    cdef str _api_token
    cdef str _account_id
    cdef object _client
    cdef bint _is_connected
    cdef set _subscribed_instruments
    cdef OandaInstrumentProvider _instrument_provider

    # cpdef TradeTick _parse_trade_tick(self, Instrument instrument, dict trade)
    cpdef Bar _parse_bar(self, Instrument instrument, dict values, PriceType price_type)
