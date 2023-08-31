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

from libc.stdint cimport uint64_t

from nautilus_trader.core.data cimport Data
from nautilus_trader.core.rust.model cimport BookOrder_t
from nautilus_trader.core.rust.model cimport OrderBookDelta_t
from nautilus_trader.model.data.book cimport OrderBookDelta
from nautilus_trader.model.data.book cimport OrderBookDeltas
from nautilus_trader.model.identifiers cimport InstrumentId


cdef class BookOrder:
    cdef BookOrder_t _mem

    cpdef double exposure(self)
    cpdef double signed_size(self)

    @staticmethod
    cdef BookOrder from_mem_c(BookOrder_t mem)

    @staticmethod
    cdef BookOrder from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(BookOrder obj)


cdef class OrderBookDelta(Data):
    cdef OrderBookDelta_t _mem

    @staticmethod
    cdef OrderBookDelta from_mem_c(OrderBookDelta_t mem)

    @staticmethod
    cdef OrderBookDelta from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(OrderBookDelta obj)

    @staticmethod
    cdef OrderBookDelta clear_c(
        InstrumentId instrument_id,
        uint64_t ts_event,
        uint64_t ts_init,
        uint64_t sequence=*,
    )


cdef class OrderBookDeltas(Data):
    cdef readonly InstrumentId instrument_id
    """The instrument ID for the order book.\n\n:returns: `InstrumentId`"""
    cdef readonly list deltas
    """The order book deltas.\n\n:returns: `list[OrderBookDelta]`"""
    cdef readonly bint is_snapshot
    """If the deltas represent a snapshot (an initial CLEAR then deltas).\n\n:returns: `bool`"""
    cdef readonly uint64_t sequence
    """If the sequence number for the last delta.\n\n:returns: `bool`"""
    cdef readonly uint64_t ts_event
    """The UNIX timestamp (nanoseconds) when the last delta event occurred.\n\n:returns: `uint64_t`"""
    cdef readonly uint64_t ts_init
    """The UNIX timestamp (nanoseconds) when the last delta event was initialized.\n\n:returns: `uint64_t`"""


    @staticmethod
    cdef OrderBookDeltas from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(OrderBookDeltas obj)
