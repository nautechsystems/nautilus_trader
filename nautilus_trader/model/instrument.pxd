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

from nautilus_trader.model.c_enums.asset_type cimport AssetType
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.objects cimport Decimal
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Quantity


cdef class Instrument:
    cdef readonly InstrumentId id
    cdef readonly Symbol symbol
    cdef readonly AssetType asset_type
    cdef readonly Currency base_currency
    cdef readonly Currency quote_currency
    cdef readonly Currency settlement_currency
    cdef readonly bint is_quanto
    cdef readonly bint is_inverse
    cdef readonly int price_precision
    cdef readonly int size_precision
    cdef readonly int cost_precision
    cdef readonly Decimal tick_size
    cdef readonly Decimal multiplier
    cdef readonly Quantity lot_size
    cdef readonly Quantity min_quantity
    cdef readonly Quantity max_quantity
    cdef readonly Money min_notional
    cdef readonly Money max_notional
    cdef readonly Decimal margin_initial
    cdef readonly Decimal margin_maintenance
    cdef readonly Decimal maker_fee
    cdef readonly Decimal taker_fee
    cdef readonly Decimal settlement_fee
    cdef readonly Decimal funding_long
    cdef readonly Decimal funding_short
    cdef readonly datetime timestamp
