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

from cpython.datetime cimport datetime

from nautilus_trader.model.identifiers cimport Symbol


cdef class OrderBook:
    cdef readonly Symbol symbol
    """The order book symbol.\n\n:returns: `Symbol`"""
    cdef readonly int level
    """The order book data level (L2, L3).\n\n:returns: `int`"""
    cdef readonly list bids
    """The bids in the order book snapshot.\n\n:returns: `list[(Price, Quantity)]`"""
    cdef readonly list asks
    """The asks in the order book snapshot.\n\n:returns: `list[(Price, Quantity)]`"""
    cdef readonly datetime timestamp
    """The order book snapshot timestamp (UTC).\n\n:returns: `datetime`"""

    @staticmethod
    cdef OrderBook from_floats(
        Symbol symbol,
        int level,
        list bids,
        list asks,
        int price_precision,
        int size_precision,
        datetime timestamp,
    )
