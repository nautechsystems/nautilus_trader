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

from nautilus_trader.backtest.models cimport SpreadQuoteAggregator
from nautilus_trader.data.client cimport DataClient
from nautilus_trader.data.client cimport MarketDataClient
from nautilus_trader.data.messages cimport SubscribeQuoteTicks
from nautilus_trader.data.messages cimport UnsubscribeQuoteTicks
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.instruments.base cimport Instrument


cdef class BacktestDataClient(DataClient):
    pass


cdef class BacktestMarketDataClient(MarketDataClient):
    cdef dict[InstrumentId, SpreadQuoteAggregator] _spread_quote_aggregators

    cdef Instrument _create_option_spread_from_components(self, InstrumentId spread_instrument_id)
    cpdef void _start_spread_quote_aggregator(self, SubscribeQuoteTicks command)
    cpdef void _stop_spread_quote_aggregator(self, UnsubscribeQuoteTicks command)
    cdef void _handle_spread_quote(self, quote)
