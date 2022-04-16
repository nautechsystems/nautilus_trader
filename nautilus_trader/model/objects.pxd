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

from libc.stdint cimport uint8_t

from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.identifiers cimport InstrumentId


cdef class BaseDecimal:
    cdef object _value

    cdef readonly uint8_t precision
    """The decimal precision.\n\n:returns: `uint8`"""

    @staticmethod
    cdef object _extract_value(object obj)

    @staticmethod
    cdef bint _compare(a, b, int op) except *

    cpdef object as_decimal(self)
    cpdef double as_double(self) except *


cdef class Quantity(BaseDecimal):
    cpdef str to_str(self)

    @staticmethod
    cdef Quantity zero_c(uint8_t precision)

    @staticmethod
    cdef Quantity from_str_c(str value)

    @staticmethod
    cdef Quantity from_int_c(int value)


cdef class Price(BaseDecimal):
    @staticmethod
    cdef Price from_str_c(str value)

    @staticmethod
    cdef Price from_int_c(int value)


cdef class Money(BaseDecimal):
    cdef readonly Currency currency
    """The currency of the money.\n\n:returns: `Currency`"""

    @staticmethod
    cdef Money from_str_c(str value)

    cpdef str to_str(self)


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
