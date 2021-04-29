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

from nautilus_trader.adapters.upbit.providers cimport UpbitInstrumentProvider
from nautilus_trader.live.data_client cimport LiveMarketDataClient
from nautilus_trader.model.bar cimport Bar
from nautilus_trader.model.bar cimport BarSpecification
from nautilus_trader.model.bar cimport BarType
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.tick cimport TradeTick


cdef class UpbitDataClient(LiveMarketDataClient):
    cdef object _client
    cdef UpbitInstrumentProvider _instrument_provider

    cdef object _subscriber

    cdef set _subscribed_instruments
    cdef dict _subscribed_order_books
    cdef dict _subscribed_quote_ticks
    cdef dict _subscribed_trade_ticks
    cdef dict _subscribed_bars

    cdef dict _market_id_to_instrument

    cdef object _handle_messages_task

    cdef object _update_instruments_task

    cdef inline void _log_ccxt_error(self, ex, str method_name) except *
    cdef inline int64_t _ccxt_to_timestamp_ns(self, int64_t millis) except *
    cdef inline void _on_bar(
        self,
        BarType bar_type,
        double open_price,
        double high_price,
        double low_price,
        double close_price,
        double volume,
        int64_t timestamp_ns,
        int price_precision,
        int size_precision,
    ) except *
    cdef inline TradeTick _parse_trade_tick(
        self,
        InstrumentId instrument_id,
        dict trade,
        int price_precision,
        int size_precision,
    )
    cdef inline Bar _parse_bar(
        self,
        BarType bar_type,
        list values,
        int price_precision,
        int size_precision,
    )
    cdef str _make_timeframe(self, BarSpecification bar_spec)
    cdef str _convert_instrument_to_market_id(self, Instrument instrument)
    cdef str _make_order_book_topic(self, str market_id)
    cdef str _make_trade_tick_topic(self, str market_id)
