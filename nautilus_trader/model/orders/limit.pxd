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

from cpython.datetime cimport datetime
from libc.stdint cimport int64_t

from nautilus_trader.model.events.order cimport OrderInitialized
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport Order


cdef class LimitOrder(Order):
    cdef readonly Price price
    """The order price (LIMIT).\n\n:returns: `Price`"""
    cdef readonly datetime expire_time
    """The order expire time.\n\n:returns: `datetime` or ``None``"""
    cdef readonly int64_t expire_time_ns
    """The order expire time (nanoseconds), zero for no expire time.\n\n:returns: `int64`"""
    cdef readonly Quantity display_qty
    """The quantity of the order to display on the public book (iceberg).\n\n:returns: `Quantity` or ``None``"""

    @staticmethod
    cdef LimitOrder create(OrderInitialized init)
