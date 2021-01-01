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

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport IdTag
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport Symbol


cdef class IdentifierGenerator:
    cdef Clock _clock
    cdef str _prefix
    cdef IdTag _id_tag_trader
    cdef IdTag _id_tag_strategy

    cdef readonly int count
    """The count of identifiers generated.\n\n:returns: `int`"""

    cpdef void set_count(self, int count) except *
    cpdef void reset(self) except *

    cdef str _generate(self)
    cdef str _get_datetime_tag(self)


cdef class OrderIdGenerator(IdentifierGenerator):
    cpdef ClientOrderId generate(self)


cdef class PositionIdGenerator:
    cdef dict _counts
    cdef IdTag _id_tag_trader

    cpdef void set_count(self, Symbol symbol, int count)
    cpdef void reset(self) except *
    cpdef PositionId generate(self, Symbol symbol, bint flipped=*)
