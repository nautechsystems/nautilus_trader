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
from nautilus_trader.core.rust.model cimport Bar_t
from nautilus_trader.core.rust.model cimport BarSpecification_t
from nautilus_trader.core.rust.model cimport BarType_t
from nautilus_trader.model.enums_c cimport BarAggregation


cdef class BarSpecification:
    cdef BarSpecification_t _mem

    cdef str to_str(self)
    cdef str aggregation_string_c(self)

    @staticmethod
    cdef BarSpecification from_mem_c(BarSpecification_t raw)

    @staticmethod
    cdef BarSpecification from_str_c(str value)

    @staticmethod
    cdef bint check_time_aggregated_c(BarAggregation aggregation)

    @staticmethod
    cdef bint check_threshold_aggregated_c(BarAggregation aggregation)

    @staticmethod
    cdef bint check_information_aggregated_c(BarAggregation aggregation)

    cpdef bint is_time_aggregated(self) except *
    cpdef bint is_threshold_aggregated(self) except *
    cpdef bint is_information_aggregated(self) except *

    @staticmethod
    cdef BarSpecification from_mem_c(BarSpecification_t raw)


cdef class BarType:
    cdef BarType_t _mem

    cdef str to_str(self)

    @staticmethod
    cdef BarType from_mem_c(BarType_t raw)

    @staticmethod
    cdef BarType from_str_c(str value)

    cpdef bint is_externally_aggregated(self) except *
    cpdef bint is_internally_aggregated(self) except *


cdef class Bar(Data):
    cdef Bar_t _mem

    cdef readonly bint is_revision
    """If this bar is a revision for a previous bar with the same `ts_event`.\n\n:returns: `bool`"""

    cdef str to_str(self)

    @staticmethod
    cdef Bar from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(Bar obj)

    cpdef bint is_single_price(self)
