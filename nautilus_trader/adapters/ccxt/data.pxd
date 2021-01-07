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


from nautilus_trader.adapters.ccxt.providers cimport CCXTInstrumentProvider
from nautilus_trader.live.data cimport LiveDataClient
from nautilus_trader.model.bar cimport Bar
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.tick cimport TradeTick


cdef class CCXTDataClient(LiveDataClient):
    cdef object _client
    cdef CCXTInstrumentProvider _instrument_provider
    cdef bint _is_connected
    cdef object _update_instruments_task

    cdef set _subscribed_instruments
    cdef dict _subscribed_trade_ticks

    cdef void _on_trade_tick(
        self,
        Instrument instrument,
        double price,
        double amount,
        str order_side,
        str liquidity_side,
        str trade_match_id,
        long timestamp,
    ) except *

    cdef inline TradeTick _parse_trade_tick(self, Instrument instrument, dict trade)
    cdef inline Bar _parse_bar(self, Instrument instrument, list values)
