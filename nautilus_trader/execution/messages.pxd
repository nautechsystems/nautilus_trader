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
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport ExecutionId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport VenueOrderId
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class OrderStatusReport:
    cdef readonly ClientOrderId client_order_id
    """The client order identifier for the report.\n\n:returns: `ClientOrderId`"""
    cdef readonly VenueOrderId venue_order_id
    """The reported venue order identifier.\n\n:returns: `VenueOrderId`"""
    cdef readonly OrderState order_state
    """The reported order state at the exchange.\n\n:returns: `OrderState`"""
    cdef readonly Quantity filled_qty
    """The reported filled quantity.\n\n:returns: `Quantity`"""
    cdef readonly int64_t timestamp_ns
    """The UNIX timestamp (nanos) of the report.\n\n:returns: `int64`"""


cdef class PositionStatusReport:
    cdef readonly InstrumentId instrument_id
    """The reported instrument identifier.\n\n:returns: `InstrumentId`"""
    cdef readonly PositionSide side
    """The reported position side at the exchange.\n\n:returns: `PositionSide`"""
    cdef readonly Quantity qty
    """The reported position quantity at the exchange.\n\n:returns: `Quantity`"""
    cdef readonly int64_t timestamp_ns
    """The UNIX timestamp (nanos) of the report.\n\n:returns: `int64`"""


cdef class ExecutionReport:
    cdef readonly ClientOrderId client_order_id
    """The client order identifier for the report.\n\n:returns: `ClientOrderId`"""
    cdef readonly VenueOrderId venue_order_id
    """The reported venue order identifier.\n\n:returns: `VenueOrderId`"""
    cdef readonly ExecutionId id
    """The reported execution identifier.\n\n:returns: `ExecutionId`"""
    cdef readonly Quantity last_qty
    """The reported quantity of the last fill.\n\n:returns: `Quantity`"""
    cdef readonly Price last_px
    """The reported price of the last fill.\n\n:returns: `Price`"""
    cdef readonly Money commission
    """The reported commission.\n\n:returns: `Money`"""
    cdef readonly LiquiditySide liquidity_side
    """The reported liquidity side.\n\n:returns: `LiquiditySide`"""
    cdef readonly int64_t ts_filled_ns
    """The UNIX timestamp (nanos) of the execution.\n\n:returns: `LiquiditySide`"""
    cdef readonly int64_t timestamp_ns
    """The UNIX timestamp (nanos) of the report.\n\n:returns: `int64`"""


cdef class ExecutionMassStatus:
    cdef dict _order_reports
    cdef dict _exec_reports
    cdef dict _position_reports

    cdef readonly ClientId client_id
    """The client identifier for the report.\n\n:returns: `ClientId`"""
    cdef readonly AccountId account_id
    """The account identifier for the report.\n\n:returns: `AccountId`"""
    cdef readonly int64_t timestamp_ns
    """The UNIX timestamp (nanos) of the report.\n\n:returns: `int64`"""

    cpdef dict order_reports(self)
    cpdef dict exec_reports(self)
    cpdef dict position_reports(self)

    cpdef void add_order_report(self, OrderStatusReport report) except *
    cpdef void add_exec_reports(self, VenueOrderId venue_order_id, list reports) except *
    cpdef void add_position_report(self, PositionStatusReport report) except *
