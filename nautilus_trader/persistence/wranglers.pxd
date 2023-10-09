# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
from libc.stdint cimport uint8_t
from libc.stdint cimport uint64_t

from nautilus_trader.model.data.bar cimport Bar
from nautilus_trader.model.data.bar cimport BarType
from nautilus_trader.model.data.book cimport OrderBookDelta
from nautilus_trader.model.data.tick cimport QuoteTick
from nautilus_trader.model.data.tick cimport TradeTick
from nautilus_trader.model.enums_c cimport AggressorSide
from nautilus_trader.model.enums_c cimport BookAction
from nautilus_trader.model.enums_c cimport OrderSide
from nautilus_trader.model.instruments.base cimport Instrument


cdef class OrderBookDeltaDataWrangler:
    cdef readonly Instrument instrument

    cpdef OrderBookDelta _build_delta_from_raw(
        self,
        BookAction action,
        OrderSide side,
        int64_t price_raw,
        uint64_t size_raw,
        uint64_t order_id,
        uint8_t flags,
        uint64_t sequence,
        uint64_t ts_event,
        uint64_t ts_init,
    )

    cpdef OrderBookDelta _build_delta(
        self,
        BookAction action,
        OrderSide side,
        double price,
        double size,
        uint64_t order_id,
        uint8_t flags,
        uint64_t sequence,
        uint64_t ts_event,
        uint64_t ts_init,
    )


cdef class QuoteTickDataWrangler:
    cdef readonly Instrument instrument

    cpdef QuoteTick _build_tick_from_raw(
        self,
        int64_t bid_price_raw,
        int64_t ask_price_raw,
        uint64_t bid_size_raw,
        uint64_t ask_size_raw,
        uint64_t ts_event,
        uint64_t ts_init,
    )

    cpdef QuoteTick _build_tick(
        self,
        double bid_price,
        double ask_price,
        double bid_size,
        double ask_size,
        uint64_t ts_event,
        uint64_t ts_init,
    )


cdef class TradeTickDataWrangler:
    cdef readonly Instrument instrument
    cdef readonly processed_data

    cpdef TradeTick _build_tick_from_raw(
        self,
        int64_t price_raw,
        uint64_t size_raw,
        AggressorSide aggressor_side,
        str trade_id,
        uint64_t ts_event,
        uint64_t ts_init,
    )

    cpdef TradeTick _build_tick(
        self,
        double price,
        double size,
        AggressorSide aggressor_side,
        str trade_id,
        uint64_t ts_event,
        uint64_t ts_init,
    )


cdef class BarDataWrangler:
    cdef readonly BarType bar_type
    cdef readonly Instrument instrument

    cpdef Bar _build_bar(self, double[:] values, uint64_t ts_event, uint64_t ts_init_delta)
