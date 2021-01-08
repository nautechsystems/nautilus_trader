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
from nautilus_trader.model.bar cimport BarSpecification
from nautilus_trader.model.bar cimport BarType
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.tick cimport TradeTick


cdef class CCXTDataClient(LiveDataClient):
    cdef object _client
    cdef CCXTInstrumentProvider _instrument_provider
    cdef bint _is_connected
    cdef object _update_instruments_task

    cdef set _subscribed_instruments
    cdef dict _subscribed_quote_ticks
    cdef dict _subscribed_trade_ticks
    cdef dict _subscribed_bars

    cdef inline void _on_quote_tick(
        self,
        Symbol symbol,
        double best_bid,
        double best_ask,
        double best_bid_size,
        double best_ask_size,
        long timestamp,
        int price_precision,
        int size_precision,
    ) except *

    cdef inline void _on_trade_tick(
        self,
        Symbol symbol,
        double price,
        double amount,
        str order_side,
        str liquidity_side,
        str trade_match_id,
        long timestamp,
        int price_precision,
        int size_precision,
    ) except *

    cdef inline void _on_bar(
        self,
        BarType bar_type,
        double open_price,
        double high_price,
        double low_price,
        double close_price,
        double volume,
        long timestamp,
        int price_precision,
        int size_precision,
    ) except *

    cdef inline TradeTick _parse_trade_tick(
        self,
        Symbol symbol,
        dict trade,
        int price_precision,
        int size_precision,
    )

    cdef inline Bar _parse_bar(
        self,
        list values,
        int price_precision,
        int size_precision,
    )

    cdef str _make_timeframe(self, BarSpecification bar_spec)
