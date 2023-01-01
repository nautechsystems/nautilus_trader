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

from nautilus_trader.model.orderbook.data cimport BookOrder


cdef class Level:
    cdef readonly double price
    """The levels price.\n\n:returns: `double`"""
    cdef readonly list orders
    """The orders at the level.\n\n:returns: `list[Order]`"""

    cpdef void bulk_add(self, list orders) except *
    cpdef void add(self, BookOrder order) except *
    cpdef void update(self, BookOrder order) except *
    cpdef void delete(self, BookOrder order) except *

    cpdef double volume(self) except *
    cpdef double exposure(self)
