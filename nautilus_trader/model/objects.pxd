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

from nautilus_trader.model.currency cimport Currency


cdef class BaseDecimal:
    cdef readonly object _value

    cdef inline object _make_decimal_with_rounding(self, value, int precision, str rounding)
    cdef inline object _make_decimal(self, double value, int precision)

    @staticmethod
    cdef inline object _extract_value(object obj)

    @staticmethod
    cdef inline bint _compare(a, b, int op) except *

    @staticmethod
    cdef inline double _eval_double(double a, double b, int op) except *

    cdef inline int precision_c(self) except *

    cpdef object as_decimal(self)
    cpdef double as_double(self) except *


cdef class Quantity(BaseDecimal):
    cpdef str to_string(self)


cdef class Price(BaseDecimal):
    pass


cdef class Money(BaseDecimal):
    cdef readonly Currency currency
    """The currency of the money.\n\n:returns: `Currency`"""

    cpdef str to_string(self)
