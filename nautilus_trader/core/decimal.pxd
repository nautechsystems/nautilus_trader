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


cdef class Decimal:
    cdef double _value

    cdef readonly int precision

    @staticmethod
    cdef Decimal zero()
    @staticmethod
    cdef Decimal from_string_to_decimal(str value)
    @staticmethod
    cdef int precision_from_string(str value)
    cpdef bint equals(self, Decimal other)
    cpdef str to_string(self, bint format_commas=*)
    cpdef int as_int(self)
    cpdef double as_double(self)
    cpdef object as_decimal(self)
    cpdef bint eq(self, Decimal other)
    cpdef bint ne(self, Decimal other)
    cpdef bint lt(self, Decimal other)
    cpdef bint le(self, Decimal other)
    cpdef bint gt(self, Decimal other)
    cpdef bint ge(self, Decimal other)
    cpdef Decimal add_decimal(self, Decimal other, bint keep_precision=*)
    cpdef Decimal subtract_decimal(self, Decimal other, bint keep_precision=*)
