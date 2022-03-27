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

from libc.stdint cimport int64_t
from libc.stdint cimport uint8_t
from libc.stdint cimport uint64_t

from nautilus_trader.core.rust.model cimport Money_t
from nautilus_trader.core.rust.model cimport Price_t
from nautilus_trader.core.rust.model cimport Quantity_t
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.identifiers cimport InstrumentId


cdef class Quantity:
    cdef Quantity_t _qty

    cdef uint64_t fixed_uint64_c(self)
    cdef double as_f64_c(self)

    cpdef str to_str(self)

    @staticmethod
    cdef object _extract_decimal(object obj)

    @staticmethod
    cdef bint _compare(a, b, int op) except *

    @staticmethod
    cdef Quantity from_fixed_c(uint64_t fixed, uint8_t precision)

    @staticmethod
    cdef Quantity zero_c(uint8_t precision)

    @staticmethod
    cdef Quantity from_str_c(str value)

    @staticmethod
    cdef Quantity from_int_c(int value)

    cpdef void add_assign(self, Quantity other) except *
    cpdef void sub_assign(self, Quantity other) except *
    cpdef object as_decimal(self)
    cpdef double as_double(self) except *


cdef class Price:
    cdef Price_t _price

    cdef int64_t fixed_int64_c(self)
    cdef double as_f64_c(self)

    @staticmethod
    cdef object _extract_decimal(object obj)

    @staticmethod
    cdef bint _compare(a, b, int op) except *

    @staticmethod
    cdef Price from_fixed_c(int64_t fixed, uint8_t precision)

    @staticmethod
    cdef Price from_str_c(str value)

    @staticmethod
    cdef Price from_int_c(int value)

    cpdef void add_assign(self, Price other) except *
    cpdef void sub_assign(self, Price other) except *
    cpdef object as_decimal(self)
    cpdef double as_double(self) except *


cdef class Money:
    cdef Money_t _money

    cdef readonly Currency currency
    """The currency of the money.\n\n:returns: `Currency`"""

    cdef int64_t fixed_int64_c(self)
    cdef double as_f64_c(self)

    @staticmethod
    cdef Money from_str_c(str value)

    cpdef str to_str(self)

    @staticmethod
    cdef object _extract_decimal(object obj)

    cpdef void add_assign(self, Money other) except *
    cpdef void sub_assign(self, Money other) except *
    cpdef object as_decimal(self)
    cpdef double as_double(self) except *


cdef class AccountBalance:
    cdef readonly Money total
    """The total account balance.\n\n:returns: `Money`"""
    cdef readonly Money locked
    """The account balance locked (assigned to pending orders).\n\n:returns: `Money`"""
    cdef readonly Money free
    """The account balance free for trading.\n\n:returns: `Money`"""
    cdef readonly Currency currency
    """The currency of the account.\n\n:returns: `Currency`"""

    @staticmethod
    cdef AccountBalance from_dict_c(dict values)
    cpdef dict to_dict(self)


cdef class MarginBalance:
    cdef readonly Money initial
    """The initial margin requirement.\n\n:returns: `Money`"""
    cdef readonly Money maintenance
    """The maintenance margin requirement.\n\n:returns: `Money`"""
    cdef readonly Currency currency
    """The currency of the margin.\n\n:returns: `Currency`"""
    cdef readonly InstrumentId instrument_id
    """The instrument ID associated with the margin.\n\n:returns: `InstrumentId` or ``None``"""

    @staticmethod
    cdef MarginBalance from_dict_c(dict values)
    cpdef dict to_dict(self)
