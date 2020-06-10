# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU General Public License Version 3.0 (the "License");
#  you may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/gpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

from nautilus_trader.model.identifiers cimport IdTag, OrderId, PositionId
from nautilus_trader.common.clock cimport Clock


cdef class IdentifierGenerator:
    cdef Clock _clock

    cdef readonly str prefix
    cdef readonly IdTag id_tag_trader
    cdef readonly IdTag id_tag_strategy
    cdef readonly int count

    cpdef void set_count(self, int count) except *
    cpdef void reset(self) except *

    cdef str _generate(self)
    cdef str _get_datetime_tag(self)


cdef class OrderIdGenerator(IdentifierGenerator):
    cpdef OrderId generate(self)


cdef class PositionIdGenerator(IdentifierGenerator):
    cpdef PositionId generate(self)
