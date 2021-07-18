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

from libc.stdint cimport int64_t

from nautilus_trader.adapters.ccxt.providers cimport CCXTInstrumentProvider
from nautilus_trader.live.data_client cimport LiveMarketDataClient
from nautilus_trader.model.data.bar cimport Bar
from nautilus_trader.model.data.bar cimport BarSpecification
from nautilus_trader.model.data.bar cimport BarType
from nautilus_trader.model.data.tick cimport TradeTick
from nautilus_trader.model.identifiers cimport InstrumentId


cdef class CCXTDataClient(LiveMarketDataClient):
    cdef object _client
    cdef CCXTInstrumentProvider _instrument_provider

    cdef set _subscribed_instruments
    cdef dict _subscribed_order_books
    cdef dict _subscribed_quote_ticks
    cdef dict _subscribed_trade_ticks
    cdef dict _subscribed_bars

    cdef object _update_instruments_task

    cdef void _log_ccxt_error(self, ex, str method_name) except *
    cdef int64_t _ccxt_to_timestamp_ns(self, int64_t millis) except *
    cdef void _on_quote_tick(
        self,
        InstrumentId instrument_id,
        double best_bid,
        double best_ask,
        double best_bid_size,
        double best_ask_size,
        int64_t ts_event_ns,
        int64_t ts_recv_ns,
        int price_precision,
        int size_precision,
    ) except *
    cdef void _on_trade_tick(
        self,
        InstrumentId instrument_id,
        double price,
        double amount,
        str aggressor_side,
        str trade_match_id,
        int64_t ts_event_ns,
        int64_t ts_recv_ns,
        int price_precision,
        int size_precision,
    ) except *
    cdef void _on_bar(
        self,
        BarType bar_type,
        double open_price,
        double high_price,
        double low_price,
        double close_price,
        double volume,
        int64_t ts_event_ns,
        int64_t ts_recv_ns,
        int price_precision,
        int size_precision,
    ) except *
    cdef TradeTick _parse_trade_tick(
        self,
        InstrumentId instrument_id,
        dict trade,
        int price_precision,
        int size_precision,
    )
    cdef Bar _parse_bar(
        self,
        BarType bar_type,
        list values,
        int price_precision,
        int size_precision,
    )
    cdef str _make_timeframe(self, BarSpecification bar_spec)
