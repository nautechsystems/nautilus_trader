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

from nautilus_trader.core.data cimport Data
from nautilus_trader.model.c_enums.book_action cimport BookAction
from nautilus_trader.model.c_enums.book_level cimport BookLevel
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.orderbook.order cimport Order


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
    cdef readonly BookAction action
    """The order book delta action {``ADD``, ``UPDATED``, ``DELETE``, ``CLEAR``}.\n\n:returns: `BookAction`"""
    cdef readonly Order order
    """The order to apply.\n\n:returns: `Order`"""

    @staticmethod
    cdef OrderBookDelta from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(OrderBookDelta obj)
