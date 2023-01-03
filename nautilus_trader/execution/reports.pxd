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

from cpython.datetime cimport datetime
from libc.stdint cimport uint64_t

from nautilus_trader.core.message cimport Document
from nautilus_trader.model.enums_c cimport ContingencyType
from nautilus_trader.model.enums_c cimport LiquiditySide
from nautilus_trader.model.enums_c cimport OrderSide
from nautilus_trader.model.enums_c cimport OrderStatus
from nautilus_trader.model.enums_c cimport OrderType
from nautilus_trader.model.enums_c cimport PositionSide
from nautilus_trader.model.enums_c cimport TimeInForce
from nautilus_trader.model.enums_c cimport TrailingOffsetType
from nautilus_trader.model.enums_c cimport TriggerType
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport OrderListId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport TradeId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.identifiers cimport VenueOrderId
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class ExecutionReport(Document):
    cdef readonly AccountId account_id
    """The account ID for the report.\n\n:returns: `AccountId`"""
    cdef readonly InstrumentId instrument_id
    """The instrument ID for the report.\n\n:returns: `InstrumentId`"""


cdef class OrderStatusReport(ExecutionReport):
    cdef readonly ClientOrderId client_order_id
    """The client order ID for the report.\n\n:returns: `ClientOrderId` or ``None``"""
    cdef readonly OrderListId order_list_id
    """The reported order list ID.\n\n:returns: `OrderListId` or ``None``"""
    cdef readonly VenueOrderId venue_order_id
    """The reported venue order ID (assigned by the venue).\n\n:returns: `VenueOrderId`"""
    cdef readonly OrderSide order_side
    """The reported order side.\n\n:returns: `OrderSide`"""
    cdef readonly OrderType order_type
    """The reported order type.\n\n:returns: `OrderType`"""
    cdef readonly ContingencyType contingency_type
    """The reported orders contingency type.\n\n:returns: `ContingencyType`"""
    cdef readonly TimeInForce time_in_force
    """The reported order time in force.\n\n:returns: `TimeInForce`"""
    cdef readonly datetime expire_time
    """The order expiration.\n\n:returns: `datetime` or ``None``"""
    cdef readonly OrderStatus order_status
    """The reported order status at the exchange.\n\n:returns: `OrderStatus`"""
    cdef readonly Price price
    """The reported order price (LIMIT).\n\n:returns: `Price` or ``None``"""
    cdef readonly Price trigger_price
    """The reported order trigger price (STOP).\n\n:returns: `Price` or ``None``"""
    cdef readonly TriggerType trigger_type
    """The trigger type for the order.\n\n:returns: `TriggerType`"""
    cdef readonly object limit_offset
    """The trailing offset for the orders limit price.\n\n:returns: `Decimal`"""
    cdef readonly object trailing_offset
    """The trailing offset for the orders trigger price (STOP).\n\n:returns: `Decimal`"""
    cdef readonly TrailingOffsetType trailing_offset_type
    """The trailing offset type.\n\n:returns: `TrailingOffsetType`"""
    cdef readonly Quantity quantity
    """The reported order original quantity.\n\n:returns: `Quantity`"""
    cdef readonly Quantity filled_qty
    """The reported filled quantity.\n\n:returns: `Quantity`"""
    cdef readonly Quantity leaves_qty
    """The reported order total leaves quantity.\n\n:returns: `Quantity`"""
    cdef readonly Quantity display_qty
    """The reported order quantity displayed on the public book (iceberg).\n\n:returns: `Quantity` or ``None``"""
    cdef readonly object avg_px
    """The reported order average fill price.\n\n:returns: `Decimal` or ``None``"""
    cdef readonly bint post_only
    """If the reported order will only provide liquidity (make a market).\n\n:returns: `bool`"""
    cdef readonly bint reduce_only
    """If the reported order carries the 'reduce-only' execution instruction.\n\n:returns: `bool`"""
    cdef readonly str cancel_reason
    """The reported reason for order cancellation.\n\n:returns: `str` or ``None``"""
    cdef readonly uint64_t ts_accepted
    """The UNIX timestamp (nanoseconds) when the reported order was accepted.\n\n:returns: `uint64_t`"""
    cdef readonly uint64_t ts_triggered
    """The UNIX timestamp (nanoseconds) when the order was triggered (0 if not triggered).\n\n:returns: `uint64_t`"""
    cdef readonly uint64_t ts_last
    """The UNIX timestamp (nanoseconds) of the last order status change.\n\n:returns: `uint64_t`"""


cdef class TradeReport(ExecutionReport):
    cdef readonly ClientOrderId client_order_id
    """The client order ID for the report.\n\n:returns: `ClientOrderId` or ``None``"""
    cdef readonly VenueOrderId venue_order_id
    """The reported venue order ID (assigned by the venue).\n\n:returns: `VenueOrderId`"""
    cdef readonly PositionId venue_position_id
    """The reported venue position ID (assigned by the venue).\n\n:returns: `PositionId` or ``None``"""
    cdef readonly TradeId trade_id
    """The reported trade match ID (assigned by the venue).\n\n:returns: `TradeId`"""
    cdef readonly OrderSide order_side
    """The reported trades side.\n\n:returns: `OrderSide`"""
    cdef readonly Quantity last_qty
    """The reported quantity of the last fill.\n\n:returns: `Quantity`"""
    cdef readonly Price last_px
    """The reported price of the last fill.\n\n:returns: `Price`"""
    cdef readonly Money commission
    """The reported commission.\n\n:returns: `Money`"""
    cdef readonly LiquiditySide liquidity_side
    """The reported liquidity side.\n\n:returns: `LiquiditySide`"""
    cdef readonly uint64_t ts_event
    """The UNIX timestamp (nanoseconds) when the execution event occurred.\n\n:returns: `LiquiditySide`"""


cdef class PositionStatusReport(ExecutionReport):
    cdef readonly PositionId venue_position_id
    """The reported venue position ID (assigned by the venue).\n\n:returns: `PositionId` or ``None``"""
    cdef readonly PositionSide position_side
    """The reported position side at the exchange.\n\n:returns: `PositionSide`"""
    cdef readonly Quantity quantity
    """The reported position quantity at the exchange.\n\n:returns: `Quantity`"""
    cdef readonly double net_qty
    """The reported net quantity (positive for ``LONG``, negative for ``SHORT``).\n\n:returns: `double`"""
    cdef readonly uint64_t ts_last
    """The UNIX timestamp (nanoseconds) of the last position change.\n\n:returns: `uint64_t`"""


cdef class ExecutionMassStatus(Document):
    cdef dict _order_reports
    cdef dict _trade_reports
    cdef dict _position_reports

    cdef readonly ClientId client_id
    """The client ID for the report.\n\n:returns: `ClientId`"""
    cdef readonly AccountId account_id
    """The account ID for the report.\n\n:returns: `AccountId`"""
    cdef readonly Venue venue
    """The venue for the report.\n\n:returns: `Venue`"""

    cpdef dict order_reports(self)
    cpdef dict trade_reports(self)
    cpdef dict position_reports(self)

    cpdef void add_order_reports(self, list reports) except *
    cpdef void add_trade_reports(self, list reports) except *
    cpdef void add_position_reports(self, list reports) except *
