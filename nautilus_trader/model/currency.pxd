# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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
from libc.stdint cimport uint16_t

from nautilus_trader.model.c_enums.currency_type cimport CurrencyType


cdef class Currency:
    cdef readonly str code
    """The currency ID code.\n\n:returns: `str`"""
    cdef readonly uint8_t precision
    """The currency decimal precision.\n\n:returns: `uint8`"""
    cdef readonly uint16_t iso4217
    """The currency ISO 4217 code.\n\n:returns: `uint16`"""
    cdef readonly str name
    """The currency name.\n\n:returns: `str`"""
    cdef readonly CurrencyType currency_type
    """The currency type {``FIAT``, ``CRYPTO``}.\n\n:returns: `CurrencyType`"""

    @staticmethod
    cdef void register_c(Currency currency, bint overwrite=*) except *

    @staticmethod
    cdef Currency from_str_c(str code)

    @staticmethod
    cdef bint is_fiat_c(str code)

    @staticmethod
    cdef bint is_crypto_c(str code)
