# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.core.rust.model cimport Currency_t
from nautilus_trader.core.rust.model cimport CurrencyType
from nautilus_trader.core.rust.model cimport Money_t
from nautilus_trader.core.rust.model cimport MoneyRaw
from nautilus_trader.core.rust.model cimport Price_t
from nautilus_trader.core.rust.model cimport PriceRaw
from nautilus_trader.core.rust.model cimport Quantity_t
from nautilus_trader.core.rust.model cimport QuantityRaw
from nautilus_trader.model.identifiers cimport InstrumentId


cdef class Quantity:
    cdef Quantity_t _mem

    cdef bint eq(self, Quantity other)
    cdef bint ne(self, Quantity other)
    cdef bint lt(self, Quantity other)
    cdef bint le(self, Quantity other)
    cdef bint gt(self, Quantity other)
    cdef bint ge(self, Quantity other)
    cdef bint is_zero(self)
    cdef bint is_negative(self)
    cdef bint is_positive(self)
    cdef QuantityRaw raw_uint_c(self)
    cdef double as_f64_c(self)

    cdef Quantity add(self, Quantity other)
    cdef Quantity sub(self, Quantity other)
    cdef void add_assign(self, Quantity other)
    cdef void sub_assign(self, Quantity other)

    @staticmethod
    cdef object _extract_decimal(object obj)

    @staticmethod
    cdef bint _compare(a, b, int op)

    @staticmethod
    cdef double raw_to_f64_c(QuantityRaw raw)

    @staticmethod
    cdef Quantity from_mem_c(Quantity_t mem)

    @staticmethod
    cdef Quantity from_raw_c(QuantityRaw raw, uint8_t precision)

    @staticmethod
    cdef Quantity zero_c(uint8_t precision)

    @staticmethod
    cdef Quantity from_str_c(str value)

    @staticmethod
    cdef Quantity from_int_c(QuantityRaw value)

    cpdef str to_formatted_str(self)
    cpdef object as_decimal(self)
    cpdef double as_double(self)


cdef class Price:
    cdef Price_t _mem

    cdef bint eq(self, Price other)
    cdef bint ne(self, Price other)
    cdef bint lt(self, Price other)
    cdef bint le(self, Price other)
    cdef bint gt(self, Price other)
    cdef bint ge(self, Price other)
    cdef bint is_zero(self)
    cdef bint is_negative(self)
    cdef bint is_positive(self)
    cdef PriceRaw raw_int_c(self)
    cdef double as_f64_c(self)

    cdef Price add(self, Price other)
    cdef Price sub(self, Price other)
    cdef void add_assign(self, Price other)
    cdef void sub_assign(self, Price other)

    @staticmethod
    cdef object _extract_decimal(object obj)

    @staticmethod
    cdef bint _compare(a, b, int op)

    @staticmethod
    cdef double raw_to_f64_c(PriceRaw raw)

    @staticmethod
    cdef Price from_mem_c(Price_t mem)

    @staticmethod
    cdef Price from_raw_c(PriceRaw raw, uint8_t precision)

    @staticmethod
    cdef Price from_str_c(str value)

    @staticmethod
    cdef Price from_int_c(PriceRaw value)

    cpdef str to_formatted_str(self)
    cpdef object as_decimal(self)
    cpdef double as_double(self)


cdef class Money:
    cdef Money_t _mem

    cdef str currency_code_c(self)
    cdef bint is_zero(self)
    cdef bint is_negative(self)
    cdef bint is_positive(self)
    cdef MoneyRaw raw_int_c(self)
    cdef double as_f64_c(self)

    @staticmethod
    cdef double raw_to_f64_c(MoneyRaw raw)

    @staticmethod
    cdef Money from_raw_c(MoneyRaw raw, Currency currency)

    @staticmethod
    cdef Money from_str_c(str value)

    @staticmethod
    cdef object _extract_decimal(object obj)

    cdef Money add(self, Money other)
    cdef Money sub(self, Money other)
    cdef void add_assign(self, Money other)
    cdef void sub_assign(self, Money other)

    cpdef str to_formatted_str(self)
    cpdef object as_decimal(self)
    cpdef double as_double(self)


cdef class Currency:
    cdef Currency_t _mem

    cdef uint8_t get_precision(self)

    @staticmethod
    cdef void register_c(Currency currency, bint overwrite=*)

    @staticmethod
    cdef Currency from_internal_map_c(str code)

    @staticmethod
    cdef Currency from_str_c(str code, bint strict=*)

    @staticmethod
    cdef bint is_fiat_c(str code)

    @staticmethod
    cdef bint is_crypto_c(str code)


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


cdef inline Price_t price_new(PriceRaw raw, uint8_t precision):
    cdef Price_t price
    price.raw = raw
    price.precision = precision
    return price


cdef inline Quantity_t quantity_new(QuantityRaw raw, uint8_t precision):
    cdef Quantity_t qty
    qty.raw = raw
    qty.precision = precision
    return qty
