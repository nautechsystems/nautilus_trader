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

from libc.stdint cimport int64_t

from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.c_enums.order_state cimport OrderState
from nautilus_trader.model.c_enums.position_side cimport PositionSide
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport ExecutionId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport OrderId
from nautilus_trader.model.objects cimport Quantity


cdef class OrderStatusReport:
    cdef readonly ClientOrderId cl_ord_id
    """The reported client order identifier.\n\n:returns: `ClientOrderId`"""
    cdef readonly OrderId order_id
    """The reported order identifier.\n\n:returns: `OrderId`"""
    cdef readonly OrderState order_state
    """The reported order state at the exchange.\n\n:returns: `OrderState`"""
    cdef readonly Quantity filled_qty
    """The reported filled quantity.\n\n:returns: `Quantity`"""
    cdef readonly int64_t timestamp_ns
    """The Unix timestamp (nanos) of the report.\n\n:returns: `int64`"""


cdef class PositionStatusReport:
    cdef readonly InstrumentId instrument_id
    """The reported instrument identifier.\n\n:returns: `InstrumentId`"""
    cdef readonly PositionSide side
    """The reported position side at the exchange.\n\n:returns: `PositionSide`"""
    cdef readonly Quantity qty
    """The reported position quantity at the exchange.\n\n:returns: `Quantity`"""
    cdef readonly int64_t timestamp_ns
    """The Unix timestamp (nanos) of the report.\n\n:returns: `int64`"""


cdef class ExecutionReport:
    # TODO: Docs
    cdef readonly ClientOrderId cl_ord_id
    cdef readonly OrderId order_id
    cdef readonly ExecutionId id
    cdef readonly object last_qty
    cdef readonly object cum_qty
    cdef readonly object leaves_qty
    cdef readonly object last_px
    cdef readonly object commission_amount
    cdef readonly str commission_currency
    cdef readonly LiquiditySide liquidity_side
    cdef readonly int64_t execution_ns
    cdef readonly int64_t timestamp_ns
    """The Unix timestamp (nanos) of the report.\n\n:returns: `int64`"""


cdef class ExecutionMassStatus:
    cdef dict _order_reports
    cdef dict _exec_reports
    cdef dict _position_reports

    cdef readonly str client
    """The client name for the report.\n\n:returns: `str`"""
    cdef readonly AccountId account_id
    """The account identifier for the report.\n\n:returns: `AccountId`"""
    cdef readonly int64_t timestamp_ns
    """The Unix timestamp (nanos) of the report.\n\n:returns: `int64`"""

    cpdef dict order_reports(self)
    cpdef dict exec_reports(self)
    cpdef dict position_reports(self)

    cpdef void add_order_report(self, OrderStatusReport report) except *
    cpdef void add_exec_reports(self, OrderId order_id, list reports) except *
    cpdef void add_position_report(self, PositionStatusReport report) except *
