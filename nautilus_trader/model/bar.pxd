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

from nautilus_trader.model.c_enums.bar_aggregation cimport BarAggregation
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class BarSpecification:
    cdef readonly int step
    """The specified step size for bar aggregation.\n\n:returns: `int`"""
    cdef readonly BarAggregation aggregation
    """The specified aggregation method for bars.\n\n:returns: `BarAggregation` (Enum)"""
    cdef readonly PriceType price_type
    """The specified price type for bar aggregation.\n\n:returns: `PriceType` (Enum)"""

    @staticmethod
    cdef BarSpecification from_str_c(str value)
    cpdef bint is_time_aggregated(self) except *
    cpdef bint is_threshold_aggregated(self) except *
    cpdef bint is_information_aggregated(self) except *


cdef class BarType:
    cdef readonly InstrumentId instrument_id
    """The bar type instrument identifier.\n\n:returns: `InstrumentId`"""
    cdef readonly Symbol symbol
    """The bar type symbol.\n\n:returns: `Symbol`"""
    cdef readonly Venue venue
    """The bar type venue.\n\n:returns: `Venue`"""
    cdef readonly BarSpecification spec
    """The bar type specification.\n\n:returns: `BarSpecification`"""
    cdef readonly bint is_internal_aggregation
    """If bar aggregation is internal to the platform.\n\n:returns: `bool`"""

    @staticmethod
    cdef BarType from_serializable_str_c(str value, bint internal_aggregation=*)
    cpdef str to_serializable_str(self)


cdef class Bar:
    cdef readonly Price open
    """The open price of the bar.\n\n:returns: `Price`"""
    cdef readonly Price high
    """The high price of the bar.\n\n:returns: `Price`"""
    cdef readonly Price low
    """The low price of the bar.\n\n:returns: `Price`"""
    cdef readonly Price close
    """The close price of the bar.\n\n:returns: `Price`"""
    cdef readonly Quantity volume
    """The volume of the bar.\n\n:returns: `Quantity`"""
    cdef readonly datetime timestamp
    """The timestamp the bar closed at (UTC).\n\n:returns: `datetime`"""
    cdef readonly bint checked
    """If the input values were integrity checked.\n\n:returns: `bool`"""

    @staticmethod
    cdef Bar from_serializable_str_c(str value)
    cpdef str to_serializable_str(self)


cdef class BarData:
    cdef readonly BarType bar_type
    """The type of the bar.\n\n:returns: `BarType`"""
    cdef readonly Bar bar
    """The bar data.\n\n:returns: `Bar`"""
