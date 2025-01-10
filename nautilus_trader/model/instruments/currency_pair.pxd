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

from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Currency


cdef class CurrencyPair(Instrument):
    cdef readonly Currency base_currency
    """The base currency for the instrument.\n\n:returns: `Currency`"""

    @staticmethod
    cdef CurrencyPair from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(CurrencyPair obj)

    @staticmethod
    cdef CurrencyPair from_pyo3_c(pyo3_instrument)
