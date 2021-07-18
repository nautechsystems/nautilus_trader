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

from nautilus_trader.model.c_enums.book_level cimport BookLevel
from nautilus_trader.model.c_enums.delta_type cimport DeltaType
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.data.base cimport Data
from nautilus_trader.model.identifiers cimport InstrumentId


cdef class OrderBookData(Data):
    cdef readonly InstrumentId instrument_id
    """The instrument ID for the order book.\n\n:returns: `InstrumentId`"""
    cdef readonly BookLevel level
    """The order book level (L1, L2, L3).\n\n:returns: `BookLevel`"""


cdef class OrderBookSnapshot(OrderBookData):
    cdef readonly list bids
    """The snapshot bids.\n\n:returns: `list`"""
    cdef readonly list asks
    """The snapshot asks.\n\n:returns: `list`"""

    @staticmethod
    cdef OrderBookSnapshot from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(OrderBookSnapshot obj)


cdef class OrderBookDeltas(OrderBookData):
    cdef readonly list deltas
    """The order book deltas.\n\n:returns: `list[OrderBookDelta]`"""

    @staticmethod
    cdef OrderBookDeltas from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(OrderBookDeltas obj)


cdef class OrderBookDelta(OrderBookData):
    cdef readonly DeltaType type
    """The type of change (ADD, UPDATED, DELETE, CLEAR).\n\n:returns: `DeltaType`"""
    cdef readonly Order order
    """The order to apply.\n\n:returns: `Order`"""

    @staticmethod
    cdef OrderBookDelta from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(OrderBookDelta obj)


cdef class Order:
    cdef readonly double price
    """The orders price.\n\n:returns: `double`"""
    cdef readonly double size
    """The orders size.\n\n:returns: `double`"""
    cdef readonly OrderSide side
    """The orders side.\n\n:returns: `OrderSide`"""
    cdef readonly str id
    """The orders ID.\n\n:returns: `str`"""

    cpdef void update_price(self, double price) except *
    cpdef void update_size(self, double size) except *
    cpdef void update_id(self, str value) except *
    cpdef double exposure(self)
    cpdef double signed_size(self)

    @staticmethod
    cdef Order from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(Order obj)
