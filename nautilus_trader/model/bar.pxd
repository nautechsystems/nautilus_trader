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
    cdef readonly int step
    cdef readonly BarAggregation aggregation
    cdef readonly PriceType price_type

    @staticmethod
    cdef BarSpecification from_string(str value)
    cdef str aggregation_string(self)
    cdef str price_type_string(self)
    cpdef bint equals(self, BarSpecification other)
    cpdef str to_string(self)


cdef class BarType:
    cdef readonly Symbol symbol
    cdef readonly BarSpecification spec

    @staticmethod
    cdef BarType from_string(str value)
    cdef bint is_time_aggregated(self)
    cdef str aggregation_string(self)
    cdef str price_type_string(self)
    cpdef bint equals(self, BarType other)
    cpdef str to_string(self)


cdef class Bar:
    cdef readonly Price open
    cdef readonly Price high
    cdef readonly Price low
    cdef readonly Price close
    cdef readonly Quantity volume
    cdef readonly datetime timestamp
    cdef readonly bint checked

    @staticmethod
    cdef Bar from_serializable_string(str value)
    cpdef bint equals(self, Bar other)
    cpdef str to_string(self)
    cpdef str to_serializable_string(self)
