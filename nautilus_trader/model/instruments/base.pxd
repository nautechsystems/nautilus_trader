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

from nautilus_trader.core.data cimport Data
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.enums_c cimport AssetClass
from nautilus_trader.model.enums_c cimport AssetType
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.tick_scheme.base cimport TickScheme


cdef class Instrument(Data):
    cdef TickScheme _tick_scheme

    cdef readonly InstrumentId id
    """The instrument ID.\n\n:returns: `InstrumentId`"""
    cdef readonly Symbol native_symbol
    """The native/local symbol on the exchange for the instrument.\n\n:returns: `Symbol`"""
    cdef readonly AssetClass asset_class
    """The asset class of the instrument.\n\n:returns: `AssetClass`"""
    cdef readonly AssetType asset_type
    """The asset type of the instrument.\n\n:returns: `AssetType`"""
    cdef readonly Currency quote_currency
    """The quote currency for the instrument.\n\n:returns: `Currency`"""
    cdef readonly bint is_inverse
    """If the quantity is expressed in quote currency.\n\n:returns: `Currency`"""
    cdef readonly int price_precision
    """The price precision of the instrument.\n\n:returns: `int`"""
    cdef readonly int size_precision
    """The size precision of the instrument.\n\n:returns: `int`"""
    cdef readonly Price price_increment
    """The minimum price increment or tick size for the instrument.\n\n:returns: `Price`"""
    cdef readonly Quantity size_increment
    """The minimum size increment for the instrument.\n\n:returns: `Quantity`"""
    cdef readonly Quantity multiplier
    """The contract multiplier for the instrument (determines tick value).\n\n:returns: `Quantity`"""
    cdef readonly Quantity lot_size
    """The rounded lot unit size (standard/board) for the instrument.\n\n:returns: `Quantity` or ``None``"""
    cdef readonly Quantity max_quantity
    """The maximum order quantity for the instrument.\n\n:returns: `Quantity` or ``None``"""
    cdef readonly Quantity min_quantity
    """The minimum order quantity for the instrument.\n\n:returns: `Quantity` or ``None``"""
    cdef readonly Money max_notional
    """The maximum notional order value for the instrument.\n\n:returns: `Money` or ``None``"""
    cdef readonly Money min_notional
    """The minimum notional order value for the instrument.\n\n:returns: `Money` or ``None``"""
    cdef readonly Price max_price
    """The maximum printable price for the instrument.\n\n:returns: `Price` or ``None``"""
    cdef readonly Price min_price
    """The minimum printable price for the instrument.\n\n:returns: `Price` or ``None``"""
    cdef readonly object margin_init
    """The initial (order) margin rate for the instrument.\n\n:returns: `Decimal`"""
    cdef readonly object margin_maint
    """The maintenance (position) margin rate for the instrument.\n\n:returns: `Decimal`"""
    cdef readonly object maker_fee
    """The maker fee rate for the instrument.\n\n:returns: `Decimal`"""
    cdef readonly object taker_fee
    """The taker fee rate for the instrument.\n\n:returns: `Decimal`"""
    cdef readonly str tick_scheme_name
    """The tick scheme name.\n\n:returns: `str` or ``None``"""
    cdef readonly dict info
    """The raw info for the instrument.\n\n:returns: `dict[str, object]`"""

    @staticmethod
    cdef Instrument base_from_dict_c(dict values)

    @staticmethod
    cdef dict base_to_dict_c(Instrument obj)

    cpdef Currency get_base_currency(self)
    cpdef Currency get_settlement_currency(self)
    cpdef Price make_price(self, value)
    cpdef Price next_bid_price(self, double value, int num_ticks=*)
    cpdef Price next_ask_price(self, double value, int num_ticks=*)
    cpdef Quantity make_qty(self, value)
    cpdef Money notional_value(self, Quantity quantity, Price price, bint inverse_as_quote=*)
