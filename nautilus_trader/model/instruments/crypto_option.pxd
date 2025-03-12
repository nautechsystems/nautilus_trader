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

from libc.stdint cimport uint64_t

from nautilus_trader.core.rust.model cimport OptionKind
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Currency
from nautilus_trader.model.objects cimport Price


cdef class CryptoOption(Instrument):
    cdef readonly Currency underlying
    """The underlying asset for the contract.\n\n:returns: `str`"""
    cdef readonly Currency settlement_currency
    """The settlement currency for the instrument.\n\n:returns: `Currency`"""
    cdef readonly OptionKind option_kind
    """The option kind (PUT | CALL) for the contract.\n\n:returns: `OptionKind`"""
    cdef readonly Price strike_price
    """The strike price for the contract.\n\n:returns: `Price`"""
    cdef readonly uint64_t activation_ns
    """UNIX timestamp (nanoseconds) for contract activation.\n\n:returns: `unit64_t`"""
    cdef readonly uint64_t expiration_ns
    """UNIX timestamp (nanoseconds) for contract expiration.\n\n:returns: `unit64_t`"""

    @staticmethod
    cdef CryptoOption from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(CryptoOption obj)

    @staticmethod
    cdef CryptoOption from_pyo3_c(pyo3_instrument)
