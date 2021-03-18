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
from cpython.datetime cimport datetime
from decimal import Decimal

from nautilus_trader.model.c_enums.asset_class cimport AssetClass
from nautilus_trader.model.c_enums.asset_type cimport AssetType
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.c_enums.position_side cimport PositionSide
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.data cimport Data
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class Instrument(Data):
    cdef readonly InstrumentId id
    """The instrument identifier.\n\n:returns: `InstrumentId`"""
    cdef readonly Symbol symbol
    """The instrument symbol.\n\n:returns: `Symbol`"""
    cdef readonly Venue venue
    """The instrument venue.\n\n:returns: `Venue`"""
    cdef readonly AssetClass asset_class
    """The asset class of the instrument.\n\n:returns: `AssetClass`"""
    cdef readonly AssetType asset_type
    """The asset type of the instrument.\n\n:returns: `AssetType`"""
    cdef readonly Currency base_currency
    """The base currency of the instrument.\n\n:returns: `Currency` or `None`"""
    cdef readonly Currency quote_currency
    """The quote currency of the instrument.\n\n:returns: `Currency`"""
    cdef readonly Currency settlement_currency
    """The settlement currency of the instrument.\n\n:returns: `Currency`"""
    cdef readonly bint is_inverse
    """If the quantity is expressed in quote currency.\n\n:returns: `Currency`"""
    cdef readonly bint is_quanto
    """If settlement currency different to base and quote.\n\n:returns: `Currency`"""
    cdef readonly int price_precision
    """The price precision of the instrument.\n\n:returns: `int`"""
    cdef readonly int size_precision
    """The size precision of the instrument.\n\n:returns: `int`"""
    cdef readonly int cost_precision
    """The cost precision of the instrument.\n\n:returns: `int`"""
    cdef readonly object tick_size
    """The tick size of the instrument.\n\n:returns: `Decimal`"""
    cdef readonly object multiplier
    """The multiplier of the instrument.\n\n:returns: `Decimal`"""
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
    cdef readonly object margin_init
    """The initial margin rate for the instrument.\n\n:returns: `Decimal`"""
    cdef readonly object margin_maint
    """The maintenance margin rate for the instrument.\n\n:returns: `Decimal`"""
    cdef readonly object maker_fee
    """The maker fee rate for the instrument.\n\n:returns: `Decimal`"""
    cdef readonly object taker_fee
    """The taker fee rate for the instrument.\n\n:returns: `Decimal`"""

    cdef bint _is_quanto(
        self,
        Currency base_currency,
        Currency quote_currency,
        Currency settlement_currency,
    ) except *

    cpdef Money market_value(self, Quantity quantity, close_price: Decimal, leverage: Decimal=*)
    cpdef Money notional_value(self, Quantity quantity, close_price: Decimal)
    cpdef Money calculate_initial_margin(self, Quantity quantity, Price price, leverage: Decimal=*)
    cpdef Money calculate_maint_margin(
        self,
        PositionSide side,
        Quantity quantity,
        Price last,
        leverage: Decimal=*,
    )

    cpdef Money calculate_commission(
        self,
        Quantity quantity,
        avg_price: Decimal,
        LiquiditySide liquidity_side,
    )


cdef class Future(Instrument):

    cdef readonly int contract_id
    cdef readonly str last_trade_date_or_contract_month
    cdef readonly str local_symbol
    cdef readonly str trading_class
    cdef readonly str market_name
    cdef readonly str long_name
    cdef readonly str contract_month
    cdef readonly str time_zone_id
    cdef readonly str trading_hours
    cdef readonly str liquid_hours
    cdef readonly str last_trade_time


cdef class BettingInstrument(Instrument):
    cdef readonly str event_type_id
    cdef readonly str event_type_name
    cdef readonly str competition_id
    cdef readonly str competition_name
    cdef readonly str event_id
    cdef readonly str event_name
    cdef readonly str event_country_code
    cdef readonly datetime event_open_date
    cdef readonly str betting_type
    cdef readonly str market_id
    cdef readonly str market_name
    cdef readonly datetime market_start_time
    cdef readonly str market_type
    cdef readonly str selection_id
    cdef readonly str selection_name
    cdef readonly str selection_handicap
