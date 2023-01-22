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

from nautilus_trader.core.data cimport Data
from nautilus_trader.core.rust.model cimport QuoteTick_t
from nautilus_trader.core.rust.model cimport TradeTick_t
from nautilus_trader.model.enums_c cimport AggressorSide
from nautilus_trader.model.enums_c cimport PriceType
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport TradeId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class QuoteTick(Data):
    cdef QuoteTick_t _mem

    cdef str to_str(self)

    @staticmethod
    cdef QuoteTick from_raw_c(
        InstrumentId instrument_id,
        int64_t raw_bid,
        int64_t raw_ask,
        uint8_t bid_price_prec,
        uint8_t ask_price_prec,
        uint64_t raw_bid_size,
        uint64_t raw_ask_size,
        uint8_t bid_size_prec,
        uint8_t ask_size_prec,
        uint64_t ts_event,
        uint64_t ts_init,
    )

    @staticmethod
    cdef QuoteTick from_mem_c(QuoteTick_t mem)

    @staticmethod
    cdef list capsule_to_quote_tick_list(object capsule)

    @staticmethod
    cdef QuoteTick from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(QuoteTick obj)

    cpdef Price extract_price(self, PriceType price_type)
    cpdef Quantity extract_volume(self, PriceType price_type)


cdef class TradeTick(Data):
    cdef TradeTick_t _mem

    cdef str to_str(self)

    @staticmethod
    cdef TradeTick from_raw_c(
        InstrumentId instrument_id,
        int64_t raw_price,
        uint8_t price_prec,
        uint64_t raw_size,
        uint8_t size_prec,
        AggressorSide aggressor_side,
        TradeId trade_id,
        uint64_t ts_event,
        uint64_t ts_init,
    )

    @staticmethod
    cdef TradeTick from_mem_c(TradeTick_t mem)

    @staticmethod
    cdef list capsule_to_trade_tick_list(object capsule)

    @staticmethod
    cdef TradeTick from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(TradeTick obj)
