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

from nautilus_trader.core.decimal cimport Decimal64
from nautilus_trader.model.c_enums.currency cimport Currency


cdef class Quantity(Decimal64):
    cpdef bint equals(self, Quantity other)

    @staticmethod
    cdef Quantity zero()

    @staticmethod
    cdef Quantity one()

    @staticmethod
    cdef Quantity from_string(str value)
    cpdef Quantity add(self, Quantity other)
    cpdef Quantity sub(self, Quantity other)
    cpdef str to_string_formatted(self)


cdef class Price(Decimal64):
    cpdef bint equals(self, Price other)

    @staticmethod
    cdef Price from_string(str value)
    cpdef Price add(self, Decimal64 other)
    cpdef Price sub(self, Decimal64 other)


cdef class Money(Decimal64):
    cdef readonly Currency currency

    cpdef bint equals(self, Money other)

    @staticmethod
    cdef Money from_string(str value, Currency currency)
    cpdef Money add(self, Money other)
    cpdef Money sub(self, Money other)
    cpdef str to_string_formatted(self)
