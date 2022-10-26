# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.model.c_enums.depth_type cimport DepthType
from nautilus_trader.model.orderbook.data cimport Order
from nautilus_trader.model.orderbook.level cimport Level


cdef class Ladder:
    cdef dict _order_id_level_index

    cdef readonly list levels
    """The ladders levels.\n\n:returns: `list[Level]`"""
    cdef readonly bint is_reversed
    """If the ladder is in reverse order.\n\n:returns: `bool`"""
    cdef readonly uint8_t price_precision
    """The ladders price precision.\n\n:returns: `uint8`"""
    cdef readonly uint8_t size_precision
    """The ladders size precision.\n\n:returns: `uint8`"""

    cpdef void add(self, Order order) except *
    cpdef void update(self, Order order) except *
    cpdef void delete(self, Order order) except *
    cpdef list depth(self, int n=*)
    cpdef list prices(self)
    cpdef list volumes(self)
    cpdef list exposures(self)
    cpdef Level top(self)
    cpdef list simulate_order_fills(self, Order order, DepthType depth_type=*)
