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

from nautilus_trader.common.component cimport Clock
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport OrderListId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId


cdef class IdentifierGenerator:
    cdef Clock _clock
    cdef str _id_tag_trader

    cdef str _get_datetime_tag(self)


cdef class ClientOrderIdGenerator(IdentifierGenerator):
    cdef str _id_tag_strategy

    cdef readonly int count
    """The count of IDs generated.\n\n:returns: `int`"""

    cpdef void set_count(self, int count)
    cpdef ClientOrderId generate(self)
    cpdef void reset(self)


cdef class OrderListIdGenerator(IdentifierGenerator):
    cdef str _id_tag_strategy

    cdef readonly int count
    """The count of IDs generated.\n\n:returns: `int`"""

    cpdef void set_count(self, int count)
    cpdef OrderListId generate(self)
    cpdef void reset(self)


cdef class PositionIdGenerator(IdentifierGenerator):
    cdef dict _counts

    cpdef void set_count(self, StrategyId strategy_id, int count)
    cpdef int get_count(self, StrategyId strategy_id)
    cpdef PositionId generate(self, StrategyId strategy_id, bint flipped=*)
    cpdef void reset(self)
