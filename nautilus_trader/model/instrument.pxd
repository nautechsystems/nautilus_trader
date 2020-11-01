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
    cdef readonly Symbol symbol
    """The symbol of the instrument.\n\n:returns: `Symbol`"""
    cdef readonly AssetClass asset_class
    """The asset class of the instrument.\n\n:returns: `AssetClass`"""
    cdef readonly AssetType asset_type
    """The asset type of the instrument.\n\n:returns: `AssetType`"""
    cdef readonly Currency base_currency
    """The base currency of the instrument.\n\n:returns: `Currency`"""
    cdef readonly Currency quote_currency
    """The quote currency of the instrument.\n\n:returns: `Currency`"""
    cdef readonly Currency settlement_currency
    """The settlement currency of the instrument.\n\n:returns: `Currency`"""
    cdef readonly bint is_inverse
    """If the instrument is inverse (quantity expressed as quote currency).\n\n:returns: `bool`"""
    cdef readonly bint is_quanto
    """If the instrument is quanto.\n\n:returns: `bool`"""
    cdef readonly int price_precision
    """The price precision of the instrument.\n\n:returns: `int`"""
    cdef readonly int size_precision
    """The size precision of the instrument.\n\n:returns: `int`"""
    cdef readonly int cost_precision
    """The cost precision of the instrument.\n\n:returns: `int`"""
    cdef readonly Decimal tick_size
    """The tick size of the instrument.\n\n:returns: `Decimal`"""
    cdef readonly Decimal multiplier
    """The multiplier of the instrument.\n\n:returns: `Decimal`"""
    cdef readonly Decimal leverage
    """The leverage of the instrument.\n\n:returns: `Decimal`"""
    cdef readonly Quantity lot_size
    """The lot size of the instrument.\n\n:returns: `Quantity`"""
    cdef readonly Quantity max_quantity
    """The maximum order quantity for the instrument.\n\n:returns: `Quantity`"""
    cdef readonly Quantity min_quantity
    """The minimum order quantity for the instrument.\n\n:returns: `Quantity`"""
    cdef readonly Money max_notional
    """The maximum notional order value for the instrument.\n\n:returns: `Money`"""
    cdef readonly Money min_notional
    """The minimum notional order value for the instrument.\n\n:returns: `Money`"""
    cdef readonly Price max_price
    """The maximum printable price for the instrument.\n\n:returns: `Price`"""
    cdef readonly Price min_price
    """The minimum printable price for the instrument.\n\n:returns: `Price`"""
    cdef readonly Decimal margin_initial
    """The initial margin rate of the instrument.\n\n:returns: `Decimal`"""
    cdef readonly Decimal margin_maintenance
    """The maintenance margin rate of the instrument.\n\n:returns: `Decimal`"""
    cdef readonly Decimal maker_fee
    """The maker fee rate of the instrument.\n\n:returns: `Decimal`"""
    cdef readonly Decimal taker_fee
    """The taker fee rate of the instrument.\n\n:returns: `Decimal`"""
    cdef readonly Decimal settlement_fee
    """The settlement fee rate of the instrument.\n\n:returns: `Decimal`"""
    cdef readonly Decimal funding_rate_long
    """The funding rate for long positions.\n\n:returns: `Decimal`"""
    cdef readonly Decimal funding_rate_short
    """The funding rate for short positions.\n\n:returns: `Decimal`"""
    cdef readonly datetime timestamp
    """The initialization timestamp of the instrument.\n\n:returns: `datetime`"""

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
