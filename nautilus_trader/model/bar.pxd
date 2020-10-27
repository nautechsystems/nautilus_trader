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

from nautilus_trader.model.c_enums.bar_aggregation cimport BarAggregation
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class BarSpecification:
    cdef int _step
    cdef BarAggregation _aggregation
    cdef PriceType _price_type

    @staticmethod
    cdef BarSpecification from_string_c(str value)
    cdef str aggregation_string(self)
    cdef str price_type_string(self)


cdef class BarType:
    cdef Symbol _symbol
    cdef BarSpecification _spec

    @staticmethod
    cdef BarType from_string_c(str value)
    cdef bint is_time_aggregated(self) except *
    cdef str aggregation_string(self)
    cdef str price_type_string(self)


cdef class Bar:
    cdef Price _open
    cdef Price _high
    cdef Price _low
    cdef Price _close
    cdef Quantity _volume
    cdef datetime _timestamp
    cdef bint _checked

    @staticmethod
    cdef Bar from_serializable_string_c(str value)
    cpdef str to_serializable_string(self)
