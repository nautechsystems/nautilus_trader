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

from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport OrderListId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.orders.base cimport Order


cdef class OrderList:
    cdef readonly OrderListId id
    """The order list ID.\n\n:returns: `OrderListId`"""
    cdef readonly InstrumentId instrument_id
    """The instrument ID associated with the list.\n\n:returns: `InstrumentId`"""
    cdef readonly StrategyId strategy_id
    """The strategy ID associated with the list.\n\n:returns: `StrategyId`"""
    cdef readonly list orders
    """The contained orders list.\n\n:returns: `list[Order]`"""
    cdef readonly Order first
    """The first order in the list (typically the parent).\n\n:returns: `list[Order]`"""
    cdef readonly uint64_t ts_init
    """UNIX timestamp (nanoseconds) when the object was initialized.\n\n:returns: `uint64_t`"""
