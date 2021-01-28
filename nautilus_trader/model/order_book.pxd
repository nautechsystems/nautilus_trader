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

from nautilus_trader.model.identifiers cimport Symbol


cdef class OrderBook:
    cdef double[:, :] _bids
    cdef double[:, :] _asks

    cdef readonly Symbol symbol
    """The order book symbol.\n\n:returns: `Symbol`"""
    cdef readonly int level
    """The order book data level (L1, L2, L3).\n\n:returns: `int`"""
    cdef readonly int price_precision
    """The precision for the order book prices.\n\n:returns: `int`"""
    cdef readonly int size_precision
    """The precision for the order book quantities.\n\n:returns: `int`"""
    cdef readonly long timestamp
    """The last update timestamp (Unix time).\n\n:returns: `long`"""

    cpdef void update(
        self,
        double[:, :] bids,
        double[:, :] asks,
        long timestamp,
    ) except *
    cdef double[:, :] bids_c(self)
    cdef double[:, :] asks_c(self)
    cpdef list bids(self)
    cpdef list asks(self)
    cpdef list bids_as_decimals(self)
    cpdef list asks_as_decimals(self)
