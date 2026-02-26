# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

from libc.stdint cimport uint64_t

from nautilus_trader.core.rust.model cimport OrderSide
from nautilus_trader.core.rust.model cimport PriceRaw
from nautilus_trader.model.book cimport BookOrder
from nautilus_trader.model.book cimport OrderBook
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.orders.base cimport Order


cdef class FillModel:
    cdef readonly double prob_fill_on_limit
    """The probability of limit orders filling on the limit price.\n\n:returns: `bool`"""
    cdef readonly double prob_slippage
    """The probability of aggressive order execution slipping.\n\n:returns: `bool`"""

    cpdef bint is_limit_filled(self)
    cpdef bint is_slipped(self)
    cpdef bint is_limit_fillable(
        self,
        OrderSide side,
        Price price,
        PriceRaw bid_raw,
        PriceRaw ask_raw,
        bint is_bid_initialized,
        bint is_ask_initialized,
    )
    cpdef OrderBook get_orderbook_for_fill_simulation(
        self,
        Instrument instrument,
        Order order,
        Price best_bid,
        Price best_ask,
    )

    cdef bint _event_success(self, double probability)


cdef class BestPriceFillModel(FillModel):
    cpdef bint is_limit_fillable(
        self,
        OrderSide side,
        Price price,
        PriceRaw bid_raw,
        PriceRaw ask_raw,
        bint is_bid_initialized,
        bint is_ask_initialized,
    )
    cpdef OrderBook get_orderbook_for_fill_simulation(
        self,
        Instrument instrument,
        Order order,
        Price best_bid,
        Price best_ask,
    )


cdef class OneTickSlippageFillModel(FillModel):
    cpdef OrderBook get_orderbook_for_fill_simulation(
        self,
        Instrument instrument,
        Order order,
        Price best_bid,
        Price best_ask,
    )


cdef class TwoTierFillModel(FillModel):
    cpdef bint is_limit_fillable(
        self,
        OrderSide side,
        Price price,
        PriceRaw bid_raw,
        PriceRaw ask_raw,
        bint is_bid_initialized,
        bint is_ask_initialized,
    )
    cpdef OrderBook get_orderbook_for_fill_simulation(
        self,
        Instrument instrument,
        Order order,
        Price best_bid,
        Price best_ask,
    )


cdef class ProbabilisticFillModel(FillModel):
    cpdef bint is_limit_fillable(
        self,
        OrderSide side,
        Price price,
        PriceRaw bid_raw,
        PriceRaw ask_raw,
        bint is_bid_initialized,
        bint is_ask_initialized,
    )
    cpdef OrderBook get_orderbook_for_fill_simulation(
        self,
        Instrument instrument,
        Order order,
        Price best_bid,
        Price best_ask,
    )


cdef class SizeAwareFillModel(FillModel):
    cpdef bint is_limit_fillable(
        self,
        OrderSide side,
        Price price,
        PriceRaw bid_raw,
        PriceRaw ask_raw,
        bint is_bid_initialized,
        bint is_ask_initialized,
    )
    cpdef OrderBook get_orderbook_for_fill_simulation(
        self,
        Instrument instrument,
        Order order,
        Price best_bid,
        Price best_ask,
    )


cdef class LimitOrderPartialFillModel(FillModel):
    cpdef bint is_limit_fillable(
        self,
        OrderSide side,
        Price price,
        PriceRaw bid_raw,
        PriceRaw ask_raw,
        bint is_bid_initialized,
        bint is_ask_initialized,
    )
    cpdef OrderBook get_orderbook_for_fill_simulation(
        self,
        Instrument instrument,
        Order order,
        Price best_bid,
        Price best_ask,
    )


cdef class ThreeTierFillModel(FillModel):
    cpdef bint is_limit_fillable(
        self,
        OrderSide side,
        Price price,
        PriceRaw bid_raw,
        PriceRaw ask_raw,
        bint is_bid_initialized,
        bint is_ask_initialized,
    )
    cpdef OrderBook get_orderbook_for_fill_simulation(
        self,
        Instrument instrument,
        Order order,
        Price best_bid,
        Price best_ask,
    )


cdef class MarketHoursFillModel(FillModel):
    cdef bint _is_low_liquidity

    cpdef bint is_low_liquidity_period(self)
    cpdef void set_low_liquidity_period(self, bint is_low_liquidity)
    cpdef bint is_limit_fillable(
        self,
        OrderSide side,
        Price price,
        PriceRaw bid_raw,
        PriceRaw ask_raw,
        bint is_bid_initialized,
        bint is_ask_initialized,
    )
    cpdef OrderBook get_orderbook_for_fill_simulation(
        self,
        Instrument instrument,
        Order order,
        Price best_bid,
        Price best_ask,
    )


cdef class VolumeSensitiveFillModel(FillModel):
    cdef double _recent_volume

    cpdef void set_recent_volume(self, double volume)
    cpdef bint is_limit_fillable(
        self,
        OrderSide side,
        Price price,
        PriceRaw bid_raw,
        PriceRaw ask_raw,
        bint is_bid_initialized,
        bint is_ask_initialized,
    )
    cpdef OrderBook get_orderbook_for_fill_simulation(
        self,
        Instrument instrument,
        Order order,
        Price best_bid,
        Price best_ask,
    )


cdef class CompetitionAwareFillModel(FillModel):
    cdef double liquidity_factor

    cpdef bint is_limit_fillable(
        self,
        OrderSide side,
        Price price,
        PriceRaw bid_raw,
        PriceRaw ask_raw,
        bint is_bid_initialized,
        bint is_ask_initialized,
    )
    cpdef OrderBook get_orderbook_for_fill_simulation(
        self,
        Instrument instrument,
        Order order,
        Price best_bid,
        Price best_ask,
    )
