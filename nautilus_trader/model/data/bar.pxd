# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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
from nautilus_trader.core.rust.model cimport Bar_t
from nautilus_trader.core.rust.model cimport BarSpecification_t
from nautilus_trader.core.rust.model cimport BarType_t
from nautilus_trader.model.c_enums.aggregation_source cimport AggregationSource
from nautilus_trader.model.c_enums.bar_aggregation cimport BarAggregation
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class BarSpecification:

    cdef BarSpecification_t _mem

    cdef str to_str(self)


    cdef str aggregation_string_c(self)

    @staticmethod
    cdef bint check_time_aggregated_c(BarAggregation aggregation)

    @staticmethod
    cdef bint check_threshold_aggregated_c(BarAggregation aggregation)

    @staticmethod
    cdef bint check_information_aggregated_c(BarAggregation aggregation)

    @staticmethod
    cdef BarSpecification from_str_c(str value)

    cpdef bint is_time_aggregated(self) except *
    cpdef bint is_threshold_aggregated(self) except *
    cpdef bint is_information_aggregated(self) except *

    @staticmethod
    cdef BarSpecification from_raw_c(BarSpecification_t raw)

cdef class BarType:

    cdef BarType_t _mem

    cdef str to_str(self)

    @staticmethod
    cdef BarType from_str_c(str value)

    cpdef bint is_externally_aggregated(self) except *
    cpdef bint is_internally_aggregated(self) except *

    @staticmethod
    cdef BarType from_raw_c(BarType_t raw)

cdef class Bar(Data):

    cdef Bar_t _mem

    cdef readonly BarType type
    """The type of the bar.\n\n:returns: `BarType`"""
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
    cdef readonly bint checked
    """If the input values were integrity checked.\n\n:returns: `bool`"""

    cdef str to_str(self)

    @staticmethod
    cdef Bar from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(Bar obj)
