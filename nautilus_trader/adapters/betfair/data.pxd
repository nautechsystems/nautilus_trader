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

from nautilus_trader.adapters.betfair.providers cimport BetfairInstrumentProvider
from nautilus_trader.live.data_client cimport LiveMarketDataClient
from nautilus_trader.model.c_enums.orderbook_level cimport OrderBookLevel
from nautilus_trader.model.data cimport Data
from nautilus_trader.model.identifiers cimport InstrumentId


cdef class BetfairDataClient(LiveMarketDataClient):
    cdef object _client
    cdef object _stream
    cdef BetfairInstrumentProvider _instrument_provider

    cdef set _subscribed_market_ids
    cdef object _update_instruments_task

    cpdef BetfairInstrumentProvider instrument_provider(self)
    cdef inline void _log_betfair_error(self, ex, str method_name) except *
    cpdef void subscribe_order_book(self, InstrumentId instrument_id, OrderBookLevel level, int depth=*, dict kwargs=*) except *
    cpdef void unsubscribe_order_book(self, InstrumentId instrument_id) except *
    cpdef void handle_data(self, Data data)
    cpdef void _on_market_update(self, bytes raw) except *
