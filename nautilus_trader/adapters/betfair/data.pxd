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
from nautilus_trader.model.data.base cimport Data


cpdef enum SubscriptionStatus:
    UNSUBSCRIBED = 0
    PENDING_STARTUP = 1
    RUNNING = 2


cdef class BetfairDataClient(LiveMarketDataClient):
    cdef object _client
    cdef object _stream
    cdef set _subscribed_instruments
    cdef set _subscribed_market_ids
    cdef SubscriptionStatus subscription_status
    cdef BetfairInstrumentProvider _instrument_provider

    cpdef BetfairInstrumentProvider instrument_provider(self)
    cpdef void handle_data(self, Data data) except *

    cdef void _log_betfair_error(self, ex, str method_name) except *
    cpdef void _on_market_update(self, bytes raw) except *
