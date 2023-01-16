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

import cython

from libc.stdint cimport uint8_t

from nautilus_trader.model.enums_c cimport DepthType
from nautilus_trader.model.orderbook.data cimport BookOrder
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

    cpdef void add(self, BookOrder order) except *
    cpdef void update(self, BookOrder order) except *
    cpdef void delete(self, BookOrder order) except *
    cpdef list depth(self, int n=*)
    cpdef list prices(self)
    cpdef list volumes(self)
    cpdef list exposures(self)
    cpdef Level top(self)
    cpdef list simulate_order_fills(self, BookOrder order, DepthType depth_type=*)


@cython.boundscheck(False)
@cython.wraparound(False)
cdef inline int bisect_right(list a, double x, int lo = 0, hi = None) except *:
    # Return the index where to insert item x in list `a`, assuming `a` is sorted.
    # The return value `i` is such that all e in `a[:i]` have `e` <= `x`, and all `e` in
    # `a[i:]` have `e` > `x`.  So if `x` already appears in the list, `a.insert(i, x)` will
    # insert just after the rightmost `x` already there.
    # Optional args `lo` (default 0) and `hi` (default len(a)) bound the
    # slice of `a` to be searched.
    if hi is None:
        hi = len(a)
    # Note, the comparison uses "<" to match the
    # __lt__() logic in list.sort() and in heapq.
    cdef int mid
    while lo < hi:
        mid = (lo + hi) // 2
        if x < a[mid]:
            hi = mid
        else:
            lo = mid + 1
    return lo
