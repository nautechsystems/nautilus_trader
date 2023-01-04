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

from libc.stdint cimport uint8_t

from nautilus_trader.core.rust.model cimport Currency_t


cdef class Currency:
    cdef Currency_t _mem

    cdef uint8_t get_precision(self)

    @staticmethod
    cdef void register_c(Currency currency, bint overwrite=*) except *

    @staticmethod
    cdef Currency from_str_c(str code, bint strict=*)

    @staticmethod
    cdef bint is_fiat_c(str code)

    @staticmethod
    cdef bint is_crypto_c(str code)
