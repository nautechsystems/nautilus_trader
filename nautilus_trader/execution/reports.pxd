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

from cpython.datetime cimport datetime

from nautilus_trader.model.c_enums.order_state cimport OrderState
from nautilus_trader.model.c_enums.position_side cimport PositionSide
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport OrderId
from nautilus_trader.model.objects cimport Quantity


cdef class ExecutionStateReport:
    cdef dict _order_states
    cdef dict _position_states

    cdef readonly str client
    """The client name for the report.\n\n:returns: `str`"""
    cdef readonly AccountId account_id
    """The account identifier for the report.\n\n:returns: `AccountId`"""
    cdef readonly datetime timestamp
    """The timestamp for the report.\n\n:returns: `datetime`"""

    cpdef dict order_states(self)
    cpdef dict position_states(self)

    cpdef void add_order_report(self, OrderStateReport report) except *
    cpdef void add_position_report(self, PositionStateReport report) except *


cdef class OrderStateReport:
    cdef readonly ClientOrderId cl_ord_id
    """The reported client order identifier.\n\n:returns: `ClientOrderId`"""
    cdef readonly OrderId order_id
    """The reported order identifier.\n\n:returns: `OrderId`"""
    cdef readonly OrderState order_state
    """The reported order state at the exchange.\n\n:returns: `OrderState`"""
    cdef readonly Quantity filled_qty
    """The reported filled quantity.\n\n:returns: `Quantity`"""
    cdef readonly datetime timestamp
    """The report timestamp.\n\n:returns: `datetime`"""


cdef class PositionStateReport:
    cdef readonly InstrumentId instrument_id
    """The reported instrument identifier.\n\n:returns: `InstrumentId`"""
    cdef readonly PositionSide side
    """The reported position side at the exchange.\n\n:returns: `PositionSide`"""
    cdef readonly Quantity qty
    """The reported position quantity at the exchange.\n\n:returns: `Quantity`"""
    cdef readonly datetime timestamp
    """The report timestamp.\n\n:returns: `datetime`"""
