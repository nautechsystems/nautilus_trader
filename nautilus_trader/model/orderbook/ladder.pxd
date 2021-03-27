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

from nautilus_trader.model.orderbook.level cimport Level
from nautilus_trader.model.orderbook.order cimport Order


cdef class Ladder:
    cdef readonly bint reverse
    """If the ladder is in reverse order.\n\n:returns: `bool`"""
    cdef readonly list levels
    """The ladders levels.\n\n:returns: `list[Level]`"""
    cdef readonly dict order_id_levels
    """The ladders levels.\n\n:returns: `dict[str, Level]`"""

    cpdef void add(self, Order order) except *
    cpdef void update(self, Order order) except *
    cpdef void delete(self, Order order) except *
    cpdef list depth(self, int n=*)
    cpdef list prices(self)
    cpdef list volumes(self)
    cpdef Level top(self)
