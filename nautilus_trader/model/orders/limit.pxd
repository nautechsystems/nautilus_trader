# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.model.events.order cimport OrderInitialized
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport Order


cdef class LimitOrder(Order):
    cdef readonly Price price
    """The order price (LIMIT).\n\n:returns: `Price`"""
    cdef readonly uint64_t expire_time_ns
    """The order expiration (UNIX epoch nanoseconds), zero for no expiration.\n\n:returns: `uint64_t`"""
    cdef readonly Quantity display_qty
    """The quantity of the order to display on the public book (iceberg).\n\n:returns: `Quantity` or ``None``"""

    @staticmethod
    cdef LimitOrder create_c(OrderInitialized init)

    @staticmethod
    cdef LimitOrder transform(Order order, uint64_t ts_init, Price price=*)

    @staticmethod
    cdef LimitOrder from_pyo3_c(pyo3_order)
