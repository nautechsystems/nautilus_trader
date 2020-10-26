# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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

from cpython.datetime cimport datetime

from nautilus_trader.core.decimal cimport Decimal
from nautilus_trader.model.c_enums.asset_class cimport AssetClass
from nautilus_trader.model.c_enums.asset_type cimport AssetType
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.c_enums.position_side cimport PositionSide
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.tick cimport QuoteTick


cdef class Instrument:
    cdef Symbol _symbol
    cdef AssetClass _asset_class
    cdef AssetType _asset_type
    cdef Currency _base_currency
    cdef Currency _quote_currency
    cdef Currency _settlement_currency
    cdef bint _is_inverse
    cdef bint _is_quanto
    cdef int _price_precision
    cdef int _size_precision
    cdef Decimal _tick_size
    cdef Decimal _multiplier
    cdef Decimal _leverage
    cdef Quantity _lot_size
    cdef Quantity _max_quantity
    cdef Quantity _min_quantity
    cdef Money _max_notional
    cdef Money _min_notional
    cdef Price _max_price
    cdef Price _min_price
    cdef Decimal _margin_initial
    cdef Decimal _margin_maintenance
    cdef Decimal _maker_fee
    cdef Decimal _taker_fee
    cdef Decimal _settlement_fee
    cdef Decimal _funding_rate_long
    cdef Decimal _funding_rate_short
    cdef datetime _timestamp

    cpdef Money calculate_order_margin(self, Quantity quantity, Price price)
    cpdef Money calculate_position_margin(
        self,
        PositionSide side,
        Quantity quantity,
        QuoteTick last,
    )

    cpdef Money calculate_open_value(
        self,
        PositionSide side,
        Quantity quantity,
        QuoteTick last,
    )

    cpdef Money calculate_unrealized_pnl(
        self,
        PositionSide side,
        Quantity quantity,
        Decimal open_price,
        QuoteTick last,
    )

    cpdef Money calculate_pnl(
        self,
        PositionSide side,
        Quantity quantity,
        Decimal avg_open,
        Decimal avg_close,
    )

    cpdef Money calculate_commission(
        self,
        Quantity quantity,
        Decimal avg_price,
        LiquiditySide liquidity_side,
    )

    cdef inline Decimal _calculate_notional(self, Quantity quantity, Decimal close_price)
    cdef inline Price _get_close_price(self, PositionSide side, QuoteTick last)
